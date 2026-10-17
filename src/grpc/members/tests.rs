use sqlx::postgres::PgPoolOptions;
use tonic::Code;

use super::{members_service_server::MembersService as _, *};

async fn disconnected_service() -> MembersService {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    pool.close().await;
    MembersService::new(MembersRepository::new(pool))
}

fn id(value: u64) -> String {
    format!("00000000-0000-0000-0000-{value:012}")
}

#[tokio::test]
async fn rejects_invalid_requests_before_querying() {
    let service = disconnected_service().await;
    for identifier in [
        None,
        Some(Identifier::MemberId(String::new())),
        Some(Identifier::MemberId("not-a-uuid".into())),
        Some(Identifier::Email(" \t".into())),
        Some(Identifier::DiscordId(String::new())),
    ] {
        let error = service
            .get_member(Request::new(GetMemberRequest { identifier }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
    }
    for request in [
        ListMembersRequest {
            page_size: -1,
            ..Default::default()
        },
        ListMembersRequest {
            page_token: "invalid".into(),
            ..Default::default()
        },
        ListMembersRequest {
            page_token: "!".repeat(24),
            ..Default::default()
        },
        ListMembersRequest {
            page_token: URL_SAFE_NO_PAD.encode([2; 18]),
            ..Default::default()
        },
        ListMembersRequest {
            page_token: encode_page_token(Uuid::nil(), true),
            include_archived: false,
            ..Default::default()
        },
    ] {
        let error = service
            .list_members(Request::new(request))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
    }
}

#[tokio::test]
async fn database_errors_return_sanitized_statuses() {
    let service = disconnected_service().await;
    let error = service
        .get_member(Request::new(GetMemberRequest {
            identifier: Some(Identifier::MemberId(id(1))),
        }))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unavailable);
    assert_eq!(error.message(), "member storage unavailable");

    let error = service
        .list_members(Request::new(ListMembersRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unavailable);

    let error = database_status(sqlx::Error::ColumnNotFound("private column".into()));
    assert_eq!(error.code(), Code::Internal);
    assert_eq!(error.message(), "failed to query members");
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
