pub mod error;

use error::PageError;

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 100;

#[derive(Debug)]
pub struct Page {
    size: usize,
    after: Option<i32>,
    scope: String,
}

impl Page {
    pub fn new(page_size: i32, token: &str, scope: &str) -> Result<Self, PageError> {
        let size = usize::try_from(page_size)?;

        let size = match size {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };

        if scope.is_empty() || scope.contains(':') {
            return Err(PageError::InvalidScope);
        }

        let after = if token.is_empty() {
            None
        } else {
            let (token_scope, cursor) =
                token.split_once(':').ok_or(PageError::InvalidTokenFormat)?;

            if token_scope != scope {
                return Err(PageError::FilterMismatch);
            }

            if cursor.is_empty() {
                return Err(PageError::MissingCursor);
            }

            if !cursor.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(PageError::InvalidCursorDigits);
            }

            let id = cursor.parse::<i32>()?;
            if id == 0 {
                return Err(PageError::NonPositiveCursor);
            }

            Some(id)
        };

        Ok(Self {
            size,
            after,
            scope: scope.to_owned(),
        })
    }

    pub fn after(&self) -> Option<i32> {
        self.after
    }

    pub fn fetch_limit(&self) -> i64 {
        self.size as i64 + 1
    }

    pub fn finish<T>(&self, mut items: Vec<T>, id: impl FnOnce(&T) -> i32) -> (Vec<T>, String) {
        let next_page_token = if items.len() > self.size {
            items.truncate(self.size);
            format!("{}:{}", self.scope, id(&items[self.size - 1]))
        } else {
            String::new()
        };

        (items, next_page_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::{Code, Status};

    #[test]
    fn page_sizes_default_clamp_and_reject_negative_values() {
        assert_eq!(Page::new(0, "", "0").unwrap().fetch_limit(), 51);
        assert_eq!(Page::new(1, "", "0").unwrap().fetch_limit(), 2);
        assert_eq!(Page::new(i32::MAX, "", "0").unwrap().fetch_limit(), 101);

        let error = Status::from(Page::new(-1, "", "0").unwrap_err());
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), "page_size must not be negative");
    }

    #[test]
    fn tokens_round_trip_for_different_scopes_and_integer_boundaries() {
        assert_eq!(Page::new(1, "", "0").unwrap().after(), None);

        for scope in ["0", "1", "another-service"] {
            for id in [1, 10, i32::MAX] {
                let page = Page::new(1, "", scope).unwrap();
                let (items, token) = page.finish(vec![id, id], |item| *item);

                assert_eq!(items, vec![id]);
                assert_eq!(token, format!("{scope}:{id}"));
                assert_eq!(Page::new(1, &token, scope).unwrap().after(), Some(id));
            }
        }
    }

    #[test]
    fn only_an_extra_row_produces_a_next_page() {
        let page = Page::new(2, "", "0").unwrap();

        for items in [vec![], vec![1], vec![1, 2]] {
            assert_eq!(page.finish(items.clone(), |id| *id), (items, String::new()));
        }

        assert_eq!(
            page.finish(vec![1, 2, 3], |id| *id),
            (vec![1, 2], "0:2".into())
        );
    }

    #[test]
    fn typed_errors_preserve_sources_and_distinguish_configuration_failures() {
        use std::error::Error as _;

        let error = Page::new(1, "0:2147483648", "0").unwrap_err();
        assert!(matches!(error, PageError::CursorOutOfRange(_)));
        assert!(error.source().is_some());

        let error = Page::new(-1, "", "0").unwrap_err();
        assert!(matches!(error, PageError::NegativePageSize(_)));
        assert!(error.source().is_some());

        for scope in ["", "invalid:scope"] {
            let error = Page::new(1, "", scope).unwrap_err();
            assert!(matches!(error, PageError::InvalidScope));
            assert_eq!(Status::from(error).code(), Code::Internal);
        }
    }

    #[test]
    fn token_errors_explain_the_failure() {
        for (token, message) in [
            ("invalid", "page_token must have the format <scope>:<id>"),
            ("1:1", "page_token does not match the request filters"),
            ("0:", "page_token is missing its cursor ID"),
            ("0:0", "page_token cursor ID must be positive"),
            (
                "0:2147483648",
                "page_token cursor ID exceeds the maximum of 2147483647",
            ),
        ] {
            let error = Status::from(Page::new(1, token, "0").unwrap_err());
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), message);
        }

        for token in ["0:-1", "0:+1", "0:1:2", "0: 1", "0:1.5", "0:１２"] {
            let error = Status::from(Page::new(1, token, "0").unwrap_err());
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(
                error.message(),
                "page_token cursor ID must contain only decimal digits"
            );
        }
    }
}
