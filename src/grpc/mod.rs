use chrono::{DateTime, SubsecRound, Utc};
use prost_types::Timestamp;
use tonic::Status;

pub mod members;
pub mod members_admin;
pub mod metrics;
pub mod page;

pub fn database_status(error: sqlx::Error) -> Status {
    tracing::error!(%error, "database operation failed");

    match error {
        sqlx::Error::PoolClosed
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => Status::unavailable("database unavailable"),
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Status::already_exists(match db.constraint() {
                Some("Member_email_key") => "email already exists",
                Some("Member_discordId_key") => "discord_id already exists",
                _ => "record already exists",
            })
        }
        sqlx::Error::RowNotFound => Status::not_found("record not found"),
        sqlx::Error::InvalidArgument(message) => Status::invalid_argument(message),
        _ => Status::internal("database operation failed"),
    }
}

pub(crate) fn timestamp(value: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    }
}

/// Inverse of [`timestamp`] for client input; prost does not validate `Timestamp`.
pub(crate) fn parse_timestamp(value: Timestamp, field: &str) -> Result<DateTime<Utc>, Status> {
    u32::try_from(value.nanos)
        .ok()
        .and_then(|nanos| DateTime::from_timestamp(value.seconds, nanos))
        // Postgres stores microseconds; truncate so incoming and stored values compare equal.
        .map(|value| value.trunc_subsecs(6))
        .ok_or_else(|| Status::invalid_argument(format!("{field} is invalid")))
}

pub(crate) fn validate_member_id(value: i32) -> Result<(), Status> {
    if value <= 0 {
        return Err(Status::invalid_argument(
            "member_id must be a positive integer",
        ));
    }

    Ok(())
}

pub(crate) fn validate_email(value: &str) -> Result<(), Status> {
    if !email_address::EmailAddress::is_valid(value) {
        return Err(Status::invalid_argument(
            "email must be a valid email address",
        ));
    }

    Ok(())
}

pub(crate) fn validate_discord_id(value: &str) -> Result<(), Status> {
    // The digit check is needed because `parse` accepts a leading "+".
    if !(value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|v| v > 0))
    {
        return Err(Status::invalid_argument(
            "discord_id must be a positive unsigned 64-bit integer containing only digits",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;

    #[test]
    fn database_failures_have_shared_sanitized_statuses() {
        for error in [
            sqlx::Error::PoolClosed,
            sqlx::Error::PoolTimedOut,
            sqlx::Error::Io(std::io::Error::other("private connection details")),
            sqlx::Error::Tls(Box::new(std::io::Error::other(
                "private certificate details",
            ))),
        ] {
            let status = database_status(error);
            assert_eq!(status.code(), Code::Unavailable);
            assert_eq!(status.message(), "database unavailable");
        }

        let status = database_status(sqlx::Error::RowNotFound);
        assert_eq!(status.code(), Code::NotFound);
        assert_eq!(status.message(), "record not found");

        let status = database_status(sqlx::Error::ColumnNotFound("private column".into()));
        assert_eq!(status.code(), Code::Internal);
        assert_eq!(status.message(), "database operation failed");
    }

    #[test]
    fn timestamps_round_trip_at_database_precision() {
        for (seconds, nanos) in [(0, -1), (i64::MAX, 0)] {
            let error = parse_timestamp(Timestamp { seconds, nanos }, "joined_at").unwrap_err();
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), "joined_at is invalid");
        }

        for seconds in [-62_135_596_800, -1, 0, 253_402_300_799] {
            let value = parse_timestamp(
                Timestamp {
                    seconds,
                    nanos: 123_456_789,
                },
                "joined_at",
            )
            .unwrap();

            assert_eq!(
                timestamp(value),
                Timestamp {
                    seconds,
                    nanos: 123_456_000
                }
            );
        }
    }

    #[test]
    fn timestamps_preserve_subseconds_before_the_epoch() {
        let value = DateTime::parse_from_rfc3339("1969-12-31T23:59:59.123456Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(
            timestamp(value),
            Timestamp {
                seconds: -1,
                nanos: 123_456_000
            }
        );
    }
}
