use chrono::{DateTime, Utc};
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
        _ => Status::internal("database operation failed"),
    }
}

pub fn timestamp(value: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    }
}

pub fn validate_email(value: &str) -> Result<(), Status> {
    if !email_address::EmailAddress::is_valid(value) {
        return Err(Status::invalid_argument(
            "email must be a valid email address",
        ));
    }
    Ok(())
}

pub fn validate_discord_id(value: &str) -> Result<(), Status> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || !value.parse::<u64>().is_ok_and(|v| v > 0)
    {
        return Err(Status::invalid_argument(
            "discord_id must be a positive unsigned 64-bit integer containing only digits",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::database_status;
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
}
