pub mod admin;
mod reader;

use tonic::{Request, Response, Status};

use crate::{
    db::members::MembersRepository,
    domain::members::{AutocompleteField, MemberRole as StoredRole, MemberSearch},
    grpc::database_status,
    proto::members::{
        GetMemberRequest, GetMemberResponse, ListCareersRequest, ListCareersResponse,
        ListGradDatesRequest, ListGradDatesResponse, ListIndustriesRequest, ListIndustriesResponse,
        ListMembersRequest, ListMembersResponse, ListSchoolsRequest, ListSchoolsResponse, Member,
        MemberRole, SearchMembersRequest, SearchMembersResponse, members_service_server,
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

    async fn search_members(
        &self,
        request: Request<SearchMembersRequest>,
    ) -> Result<Response<SearchMembersResponse>, Status> {
        let request = request.into_inner();
        let query = request.query.trim().to_lowercase();
        if query.len() > 256 {
            return Err(Status::invalid_argument("query must be at most 256 bytes"));
        }
        let roles = search_roles(request.roles)?;
        let page_size = match request.page_size {
            0 => 10,
            size => size.min(100),
        };
        let result = self
            .repository
            .search(
                MemberSearch::new(query, roles, request.page.max(1), page_size)
                    .map_err(Status::invalid_argument)?,
            )
            .await
            .map_err(database_status)?;
        let pages = result.matching_total.div_ceil(u64::from(page_size));
        Ok(Response::new(SearchMembersResponse {
            members: result.members.into_iter().map(Into::into).collect(),
            total: result.total,
            matching_total: result.matching_total,
            page: result.page,
            page_size,
            pages,
            has_next_page: u64::from(result.page) < pages,
        }))
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
    async fn list_schools(
        &self,
        request: Request<ListSchoolsRequest>,
    ) -> Result<Response<ListSchoolsResponse>, Status> {
        let request = request.into_inner();
        let schools = self
            .autocomplete(AutocompleteField::School, request.query, request.limit)
            .await?;

        Ok(ListSchoolsResponse { schools }.into())
    }
    async fn list_grad_dates(
        &self,
        request: Request<ListGradDatesRequest>,
    ) -> Result<Response<ListGradDatesResponse>, Status> {
        let request = request.into_inner();
        let grad_dates = self
            .autocomplete(AutocompleteField::GradDate, request.query, request.limit)
            .await?;

        Ok(ListGradDatesResponse { grad_dates }.into())
    }
}

fn search_roles(roles: Vec<i32>) -> Result<Vec<StoredRole>, Status> {
    roles.into_iter().try_fold(Vec::new(), |mut roles, role| {
        let role = match MemberRole::try_from(role) {
            Ok(MemberRole::Board) => StoredRole::Board,
            Ok(MemberRole::Mentor) => StoredRole::Mentor,
            Ok(MemberRole::Mentee) => StoredRole::Mentee,
            _ => return Err(Status::invalid_argument("invalid member role")),
        };
        if !roles.contains(&role) {
            roles.push(role);
        }
        Ok(roles)
    })
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
    async fn education_autocomplete_matches_and_ranks_stored_text() {
        let mut members: Vec<_> = (1..=3).map(stored).collect();
        for (member, value) in members
            .iter_mut()
            .zip(["Spring 2028", "Spring 2028", "Spring 2029"])
        {
            member.school = Some(value.into());
            member.grad_date = Some(value.into());
        }
        let service = MembersService::new(FakeMembersRepository::Stored(members));
        let schools = service
            .list_schools(Request::new(ListSchoolsRequest {
                query: "SPRING".into(),
                limit: 1,
            }))
            .await
            .unwrap()
            .into_inner()
            .schools;
        assert_eq!(schools, ["Spring 2028"]);
        let dates = service
            .list_grad_dates(Request::new(ListGradDatesRequest {
                query: "spring".into(),
                limit: 0,
            }))
            .await
            .unwrap()
            .into_inner()
            .grad_dates;
        assert_eq!(dates, ["Spring 2028", "Spring 2029"]);
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
            school: Some("Example University".into()),
            grad_date: Some("Spring 2028".into()),
            profile: Some("admin notes".into()),
            role: None,
            preferred_communications: vec![],
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

            assert_eq!(member.school.as_deref(), Some("Example University"));
            assert_eq!(member.grad_date.as_deref(), Some("Spring 2028"));
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

                for member in &page.members {
                    assert_eq!(member.school.as_deref(), Some("Example University"));
                    assert_eq!(member.grad_date.as_deref(), Some("Spring 2028"));
                }
                ids.extend(page.members.iter().map(|member| member.member_id));
                page_token = page.next_page_token;

                if page_token.is_empty() {
                    break;
                }
            }

            assert_eq!(ids, expected);
        }
    }
    #[tokio::test]
    async fn search_filters_before_pagination_and_preserves_total() {
        let mut rows: Vec<_> = (1..=30).map(stored).collect();
        for row in &mut rows {
            row.first_name = if row.id % 2 == 0 { "Alice" } else { "Bob" }.into();
            row.role = Some(crate::domain::members::MemberRole::Mentor);
        }
        let service = MembersService::new(FakeMembersRepository::Stored(rows));
        let response = service
            .search_members(Request::new(SearchMembersRequest {
                query: " ALI ".into(),
                roles: vec![MemberRole::Mentor as i32],
                page: 2,
                page_size: 10,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.total, 29);
        assert_eq!(response.matching_total, 15);
        assert_eq!(response.page, 2);
        assert_eq!(response.pages, 2);
        assert!(!response.has_next_page);
        assert_eq!(
            response
                .members
                .iter()
                .map(|m| m.member_id)
                .collect::<Vec<_>>(),
            [10, 8, 6, 4, 2]
        );
        let empty = service
            .search_members(Request::new(SearchMembersRequest {
                query: "unknown".into(),
                page: 99,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(empty.total, 29);
        assert_eq!(empty.matching_total, 0);
        assert_eq!(empty.page, 1);
        assert_eq!(empty.pages, 0);
        assert!(!empty.has_next_page);
        assert!(empty.members.is_empty());
    }

    #[tokio::test]
    async fn search_validates_filters_before_querying() {
        for request in [
            SearchMembersRequest {
                roles: vec![0],
                ..Default::default()
            },
            SearchMembersRequest {
                roles: vec![99],
                ..Default::default()
            },
            SearchMembersRequest {
                query: "x".repeat(257),
                ..Default::default()
            },
        ] {
            let error = disconnected_service()
                .search_members(Request::new(request))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Code::InvalidArgument);
        }
        let error = disconnected_service()
            .search_members(Request::new(SearchMembersRequest::default()))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);
    }

    #[tokio::test]
    async fn search_returns_page_metadata_for_first_middle_and_last_pages() {
        let service = MembersService::new(FakeMembersRepository::Stored(
            (1..=31).map(stored).collect(),
        ));
        for (page, has_next_page) in [(1, true), (2, true), (3, false)] {
            let response = service
                .search_members(Request::new(SearchMembersRequest {
                    page,
                    page_size: 10,
                    ..Default::default()
                }))
                .await
                .unwrap()
                .into_inner();
            assert_eq!(response.pages, 3);
            assert_eq!(response.has_next_page, has_next_page);
            assert_eq!(response.page, page);
            assert_eq!(response.members.len(), 10);
        }
    }
    #[test]
    fn search_roles_deduplicate_and_still_reject_invalid_values() {
        let mentor = MemberRole::Mentor as i32;
        let board = MemberRole::Board as i32;
        assert_eq!(
            search_roles(vec![mentor, board, mentor]).unwrap(),
            [StoredRole::Mentor, StoredRole::Board]
        );
        assert_eq!(
            search_roles(vec![mentor, mentor, 0]).unwrap_err().code(),
            Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn search_normalizes_zero_pagination_before_repository_calls() {
        let service = MembersService::new(FakeMembersRepository::Stored(vec![stored(1)]));
        let response = service
            .search_members(Request::new(SearchMembersRequest::default()))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.page, 1);
        assert_eq!(response.page_size, 10);
        assert_eq!(response.members.len(), 1);
    }
}
