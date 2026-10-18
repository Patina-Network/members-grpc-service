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
        sqlx::Error::RowNotFound => Status::not_found("record not found"),
        _ => Status::internal("database operation failed"),
    }
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
