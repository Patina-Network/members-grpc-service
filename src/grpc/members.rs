use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::{
    db::members::{Member as StoredMember, MemberIdentifier, MembersRepository},
    proto::members::{
        GetMemberRequest, GetMemberResponse, ListMembersRequest, ListMembersResponse, Member,
        get_member_request::Identifier, members_service_server,
    },
};

pub use members_service_server::MembersServiceServer;

const DEFAULT_PAGE_SIZE: i32 = 50;
const MAX_PAGE_SIZE: i32 = 100;

pub struct MembersService {
    repository: MembersRepository,
}

impl MembersService {
    pub fn new(repository: MembersRepository) -> Self {
        Self { repository }
    }
}

#[tonic::async_trait]
impl members_service_server::MembersService for MembersService {
    async fn get_member(
        &self,
        request: Request<GetMemberRequest>,
    ) -> Result<Response<GetMemberResponse>, Status> {
        let identifier = match request.into_inner().identifier {
            Some(Identifier::MemberId(id)) if id > 0 => MemberIdentifier::Id(id),
            Some(Identifier::MemberId(_)) => {
                return Err(Status::invalid_argument(
                    "member_id must be a positive integer",
                ));
            }
            Some(Identifier::Email(email)) if email_address::EmailAddress::is_valid(&email) => {
                MemberIdentifier::Email(email)
            }
            Some(Identifier::DiscordId(id))
                if !id.is_empty()
                    && id.bytes().all(|byte| byte.is_ascii_digit())
                    && id.parse::<u64>().is_ok_and(|value| value > 0) =>
            {
                MemberIdentifier::DiscordId(id)
            }
            _ => {
                return Err(Status::invalid_argument(
                    "a non-empty member identifier is required",
                ));
            }
        };
        let member = self
            .repository
            .get(identifier)
            .await
            .map_err(database_status)?
            .ok_or_else(|| Status::not_found("member not found"))?;

        Ok(GetMemberResponse {
            member: Some(member.into()),
        }
        .into())
    }

    async fn list_members(
        &self,
        request: Request<ListMembersRequest>,
    ) -> Result<Response<ListMembersResponse>, Status> {
        let request = request.into_inner();
        let page_size = match request.page_size {
            size if size < 0 => {
                return Err(Status::invalid_argument("page_size must not be negative"));
            }
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        } as usize;
        let after = decode_page_token(&request.page_token, request.include_archived)?;
        let mut members = self
            .repository
            .list(request.include_archived, after, page_size as i64 + 1)
            .await
            .map_err(database_status)?;

        let next_page_token = if members.len() > page_size {
            members.truncate(page_size);
            encode_page_token(members[page_size - 1].id, request.include_archived)
        } else {
            String::new()
        };

        Ok(ListMembersResponse {
            members: members.into_iter().map(Into::into).collect(),
            next_page_token,
        }
        .into())
    }
}

fn encode_page_token(id: i32, include_archived: bool) -> String {
    format!("{}:{id}", u8::from(include_archived))
}

fn decode_page_token(token: &str, include_archived: bool) -> Result<Option<i32>, Status> {
    if token.is_empty() {
        return Ok(None);
    }
    let invalid = || Status::invalid_argument("invalid page_token for this request");
    let (filter, id) = token.split_once(':').ok_or_else(invalid)?;
    if filter != if include_archived { "1" } else { "0" }
        || id.is_empty()
        || !id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    let id = id.parse::<i32>().map_err(|_| invalid())?;
    if id <= 0 {
        return Err(invalid());
    }
    Ok(Some(id))
}

fn database_status(error: sqlx::Error) -> Status {
    tracing::error!(%error, "member query failed");
    match error {
        sqlx::Error::PoolClosed
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => Status::unavailable("member storage unavailable"),
        _ => Status::internal("failed to query members"),
    }
}

impl From<StoredMember> for Member {
    fn from(member: StoredMember) -> Self {
        Self {
            member_id: member.id,
            email: member.email,
            first_name: member.first_name,
            last_name: member.last_name,
            joined_at: Some(timestamp(member.joined_at)),
            discord_id: member.discord_id,
            discord_username: member.discord_username,
            updated_at: Some(timestamp(member.updated_at)),
            archived_at: member.archived_at.map(timestamp),
        }
    }
}

fn timestamp(value: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn page_tokens_are_readable_and_validate_filter_and_integer_range() {
        assert_eq!(decode_page_token("", false).unwrap(), None);
        for include_archived in [false, true] {
            for id in [1, 10, i32::MAX] {
                let token = encode_page_token(id, include_archived);
                assert_eq!(token, format!("{}:{id}", u8::from(include_archived)));
                assert_eq!(
                    decode_page_token(&token, include_archived).unwrap(),
                    Some(id)
                );
                assert!(decode_page_token(&token, !include_archived).is_err());
            }
        }
        for token in [
            "0:0",
            "0:-1",
            "0:+1",
            "0:2147483648",
            "0:",
            "0:1:2",
            "0: 1",
            "2:1",
            "1",
            "0:1.5",
        ] {
            assert_eq!(
                decode_page_token(token, false).unwrap_err().code(),
                Code::InvalidArgument
            );
        }
    }

    #[tokio::test]
    async fn integer_ids_and_pagination_against_postgres() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query(r#"CREATE TEMP TABLE "Member" (LIKE public."Member" INCLUDING ALL)"#)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            r#"
            INSERT INTO "Member"
                (email, "firstName", "lastName", "joinedAt", "updatedAt", "discordId", "archivedAt")
            SELECT
                'test' || n || '@example.com', 'Test', 'Member', NOW(), NOW(),
                n::text, CASE WHEN n = 12 THEN NOW() END
            FROM
                generate_series(1, 12) AS n
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let service = MembersService::new(MembersRepository::new(pool.clone()));
        for identifier in [
            Identifier::MemberId(10),
            Identifier::Email("test10@example.com".into()),
            Identifier::DiscordId("10".into()),
        ] {
            let member = service
                .get_member(Request::new(GetMemberRequest {
                    identifier: Some(identifier),
                }))
                .await
                .unwrap()
                .into_inner()
                .member
                .unwrap();
            assert_eq!(member.member_id, 10);
        }
        for include_archived in [false, true] {
            let mut token = String::new();
            let mut ids = Vec::new();
            loop {
                let response = service
                    .list_members(Request::new(ListMembersRequest {
                        include_archived,
                        page_size: 5,
                        page_token: token,
                    }))
                    .await
                    .unwrap()
                    .into_inner();
                ids.extend(response.members.iter().map(|member| member.member_id));
                token = response.next_page_token;
                if token.is_empty() {
                    break;
                }
                assert!(ids.len() < 12);
            }
            assert_eq!(
                ids,
                (1..=if include_archived { 12 } else { 11 }).collect::<Vec<_>>()
            );
        }
        let error = service
            .get_member(Request::new(GetMemberRequest {
                identifier: Some(Identifier::MemberId(100)),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::NotFound);
        pool.close().await;
    }

    #[tokio::test]
    async fn rejects_invalid_requests_before_querying() {
        let service = disconnected_service().await;
        for identifier in [
            None,
            Some(Identifier::MemberId(0)),
            Some(Identifier::MemberId(-1)),
            Some(Identifier::Email(" \t".into())),
            Some(Identifier::Email("not-an-email".into())),
            Some(Identifier::Email("member@".into())),
            Some(Identifier::DiscordId("abc".into())),
            Some(Identifier::DiscordId("-1".into())),
            Some(Identifier::DiscordId("+1".into())),
            Some(Identifier::DiscordId("1.5".into())),
            Some(Identifier::DiscordId("0".into())),
            Some(Identifier::DiscordId("18446744073709551616".into())),
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
                page_token: "2:1".into(),
                ..Default::default()
            },
            ListMembersRequest {
                page_token: encode_page_token(1, true),
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
                identifier: Some(Identifier::MemberId(1)),
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

    #[tokio::test]
    async fn valid_identifiers_reach_the_repository() {
        let service = disconnected_service().await;
        for identifier in [
            Identifier::MemberId(1),
            Identifier::Email("member+tag@example.com".into()),
            Identifier::DiscordId("18446744073709551615".into()),
        ] {
            let error = service
                .get_member(Request::new(GetMemberRequest {
                    identifier: Some(identifier),
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Code::Unavailable);
        }
    }
}
