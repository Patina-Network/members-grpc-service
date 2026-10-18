use std::num::{ParseIntError, TryFromIntError};

use thiserror::Error;
use tonic::Status;

#[derive(Debug, Error)]
pub enum PageError {
    #[error("page_size must not be negative")]
    NegativePageSize(#[from] TryFromIntError),
    #[error("invalid pagination scope configuration")]
    InvalidScope,
    #[error("page_token must have the format <scope>:<id>")]
    InvalidTokenFormat,
    #[error("page_token does not match the request filters")]
    FilterMismatch,
    #[error("page_token is missing its cursor ID")]
    MissingCursor,
    #[error("page_token cursor ID must contain only decimal digits")]
    InvalidCursorDigits,
    #[error("page_token cursor ID exceeds the maximum of 2147483647")]
    CursorOutOfRange(#[from] ParseIntError),
    #[error("page_token cursor ID must be positive")]
    NonPositiveCursor,
}

impl From<PageError> for Status {
    fn from(error: PageError) -> Self {
        match error {
            PageError::InvalidScope => {
                tracing::error!(%error, "pagination configuration failed");
                Self::internal(error.to_string())
            }
            _ => Self::invalid_argument(error.to_string()),
        }
    }
}
