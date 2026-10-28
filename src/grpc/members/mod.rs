pub mod admin;
mod reader;

use tonic::{Request, Response, Status};

use crate::{
    db::members::MembersRepository,
    domain::members::AutocompleteField,
    grpc::database_status,
    proto::members::{
        GetMemberRequest, GetMemberResponse, ListCareersRequest, ListCareersResponse,
        ListIndustriesRequest, ListIndustriesResponse, ListMembersRequest, ListMembersResponse,
        Member, members_service_server,
    },
};

use reader::MembersReader;

const DEFAULT_AUTOCOMPLETE_LIMIT: u32 = 10;
const MAX_AUTOCOMPLETE_LIMIT: u32 = 50;

pub use members_service_server::MembersServiceServer;

pub struct MembersService<R> {
    repository: R,
    reader: MembersReader<R, Member>,
}

impl<R: MembersRepository> MembersService<R> {
    pub fn new(repository: R) -> Self
    where
        R: Clone,
    {
        Self {
            reader: MembersReader::new(repository.clone()),
            repository,
        }
    }

    async fn autocomplete(
        &self,
        field: AutocompleteField,
        query: String,
        limit: u32,
    ) -> Result<Vec<String>, Status> {
        let limit = match limit {
            0 => DEFAULT_AUTOCOMPLETE_LIMIT,
            limit => limit.min(MAX_AUTOCOMPLETE_LIMIT),
        };

        self.repository
            .autocomplete(field, query, i64::from(limit))
            .await
            .map_err(database_status)
    }
}

#[tonic::async_trait]
impl<R: MembersRepository> members_service_server::MembersService for MembersService<R> {
    async fn get_member(
        &self,
        request: Request<GetMemberRequest>,
    ) -> Result<Response<GetMemberResponse>, Status> {
        let member = self.reader.get(request.into_inner()).await?;

        Ok(GetMemberResponse {
            member: Some(member),
        }
        .into())
    }

    async fn list_members(
        &self,
        request: Request<ListMembersRequest>,
    ) -> Result<Response<ListMembersResponse>, Status> {
        let (members, next_page_token) = self.reader.list(request.into_inner()).await?;

        Ok(ListMembersResponse {
            members,
            next_page_token,
        }
        .into())
    }

    async fn list_careers(
        &self,
        request: Request<ListCareersRequest>,
    ) -> Result<Response<ListCareersResponse>, Status> {
        let request = request.into_inner();
        let careers = self
            .autocomplete(AutocompleteField::Career, request.query, request.limit)
            .await?;

        Ok(ListCareersResponse { careers }.into())
    }

    async fn list_industries(
        &self,
        request: Request<ListIndustriesRequest>,
    ) -> Result<Response<ListIndustriesResponse>, Status> {
        let request = request.into_inner();
        let industries = self
            .autocomplete(AutocompleteField::Industry, request.query, request.limit)
            .await?;

        Ok(ListIndustriesResponse { industries }.into())
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use tonic::Code;

    use crate::{
        db::members::fake::FakeMembersRepository, domain::members::Member as StoredMember,
        proto::members::get_member_request::Identifier,
    };

    use super::{members_service_server::MembersService as _, *};

    fn disconnected_service() -> MembersService<FakeMembersRepository> {
        MembersService::new(FakeMembersRepository::Unavailable)
    }

    #[tokio::test]
    async fn rejects_invalid_requests_before_querying() {
        let service = disconnected_service();

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
                page_token: "1:1".into(),
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
    async fn invalid_identifiers_report_the_field_and_reason() {
        let service = disconnected_service();

        for (identifier, message) in [
            (None, "a member identifier is required"),
            (
                Some(Identifier::MemberId(0)),
                "member_id must be a positive integer",
            ),
            (
                Some(Identifier::Email("invalid".into())),
                "email must be a valid email address",
            ),
            (
                Some(Identifier::DiscordId("abc".into())),
                "discord_id must be a positive unsigned 64-bit integer containing only digits",
            ),
        ] {
            let error = service
                .get_member(Request::new(GetMemberRequest { identifier }))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), message);
        }
    }

    #[tokio::test]
    async fn database_errors_return_sanitized_statuses() {
        let service = disconnected_service();

        let error = service
            .get_member(Request::new(GetMemberRequest {
                identifier: Some(Identifier::MemberId(1)),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);
        assert_eq!(error.message(), "database unavailable");

        let error = service
            .list_members(Request::new(ListMembersRequest::default()))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);

        let error = database_status(sqlx::Error::ColumnNotFound("private column".into()));
        assert_eq!(error.code(), Code::Internal);
        assert_eq!(error.message(), "database operation failed");
    }

    #[tokio::test]
    async fn valid_identifiers_reach_the_repository() {
        let service = disconnected_service();

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

    fn stored(id: i32) -> StoredMember {
        StoredMember {
            id,
            email: format!("m{id}@example.org"),
            first_name: "First".into(),
            last_name: "Last".into(),
            joined_at: DateTime::UNIX_EPOCH,
            discord_id: Some(id.to_string()),
            discord_username: Some(format!("member{id}")),
            linkedin_username: None,
            instagram_username: None,
            career: None,
            industry: None,
            profile: Some("admin notes".into()),
            role: None,
            updated_at: DateTime::UNIX_EPOCH,
            archived_at: (id == 3).then_some(DateTime::UNIX_EPOCH),
        }
    }

    #[tokio::test]
    async fn stored_members_are_found_and_paged() {
        let service =
            MembersService::new(FakeMembersRepository::Stored((1..=3).map(stored).collect()));

        for identifier in [
            Identifier::MemberId(2),
            Identifier::Email("m2@example.org".into()),
            Identifier::DiscordId("2".into()),
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

            assert_eq!(member.member_id, 2);
            assert_eq!(member.discord_username.as_deref(), Some("member2"));
            assert_eq!(member.joined_at, Some(prost_types::Timestamp::default()));
            assert_eq!(member.archived_at, None);
        }

        let error = service
            .get_member(Request::new(GetMemberRequest {
                identifier: Some(Identifier::MemberId(9)),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::NotFound);
        assert_eq!(error.message(), "member not found");

        for (include_archived, expected) in [(false, vec![1, 2]), (true, vec![1, 2, 3])] {
            let mut ids = Vec::new();
            let mut page_token = String::new();

            loop {
                let page = service
                    .list_members(Request::new(ListMembersRequest {
                        include_archived,
                        page_size: 1,
                        page_token,
                    }))
                    .await
                    .unwrap()
                    .into_inner();

                ids.extend(page.members.iter().map(|member| member.member_id));
                page_token = page.next_page_token;

                if page_token.is_empty() {
                    break;
                }
            }

            assert_eq!(ids, expected);
        }
    }
}
