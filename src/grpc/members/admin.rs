use std::time::Duration;

use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::{
    db::members::MembersRepository,
    domain::members::{
        CreateMember, Member as StoredMember, MemberIdentifier, MemberRole, OperationOutcome,
        UpdateMember, UpsertWrite,
    },
    grpc::{
        database_status, parse_timestamp, validate_discord_id, validate_email, validate_member_id,
    },
    proto::members::{
        GetMemberRequest, ListMembersRequest, MemberRole as ProtoRole,
        admin::{
            ArchiveMemberRequest, ArchiveMemberResponse, BulkMemberOperationsRequest,
            BulkMemberOperationsResponse, CreateMemberRequest, CreateMemberResponse,
            GetSensitiveMemberResponse, ListSensitiveMembersResponse, MemberArchived,
            MemberCreated, MemberOperation, MemberOperationFailed, MemberOperationResult,
            MemberUnchanged, MemberUpdated, SensitiveMember, UpdateMemberRequest,
            UpdateMemberResponse, member_operation::Operation, member_operation_result::Outcome,
            members_admin_service_server,
        },
    },
};

use super::reader::MembersReader;

pub use members_admin_service_server::MembersAdminServiceServer;

pub struct MembersAdminService<R> {
    repository: R,
    reader: MembersReader<R, SensitiveMember>,
    max_bulk_operations: usize,
    operation_timeout: Duration,
}

impl<R: MembersRepository> MembersAdminService<R> {
    pub fn new(repository: R, max_bulk_operations: usize, operation_timeout: Duration) -> Self
    where
        R: Clone,
    {
        Self {
            reader: MembersReader::new(repository.clone()),
            repository,
            max_bulk_operations,
            operation_timeout,
        }
    }

    async fn apply_operation(&self, input: MemberOperation) -> Result<Outcome, Status> {
        match input
            .operation
            .ok_or_else(|| Status::invalid_argument("operation is required"))?
        {
            Operation::Create(input) => self
                .repository
                .create_member(input.try_into()?)
                .await
                .map(|member| OperationOutcome::Created(member).into())
                .map_err(database_status),
            Operation::Update(input) => self
                .repository
                .update_operation(input.try_into()?)
                .await
                .map(Into::into)
                .map_err(database_status),
            Operation::Archive(input) => {
                validate_member_id(input.member_id)?;
                self.repository
                    .archive_operation(input.member_id)
                    .await
                    .map(Into::into)
                    .map_err(database_status)
            }
            Operation::Upsert(input) => {
                let member = CreateMember::try_from(
                    input
                        .member
                        .ok_or_else(|| Status::invalid_argument("upsert member is required"))?,
                )?;

                let existing_id = self
                    .repository
                    .get(MemberIdentifier::Email(member.email.clone()))
                    .await
                    .map_err(database_status)?
                    .map(|existing| existing.id);

                match UpsertWrite::new(member, existing_id) {
                    UpsertWrite::Update(update) => self.repository.update_operation(update).await,
                    UpsertWrite::Create(create) => self
                        .repository
                        .create_member(create)
                        .await
                        .map(OperationOutcome::Created),
                }
                .map(Into::into)
                .map_err(database_status)
            }
        }
    }
}

fn required(value: &str, field: &str) -> Result<(), Status> {
    if value.trim().is_empty() {
        return Err(Status::invalid_argument(format!("{field} is required")));
    }
    Ok(())
}

fn validate_username(value: &str, field: &str) -> Result<(), Status> {
    required(value, field)?;
    if value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '/' | '@' | '?' | '#'))
    {
        return Err(Status::invalid_argument(format!(
            "{field} must be a bare username, not a URL or @handle"
        )));
    }
    Ok(())
}

fn validate_optional(
    value: &Option<String>,
    validate: impl FnOnce(&str) -> Result<(), Status>,
) -> Result<(), Status> {
    value.as_deref().map_or(Ok(()), validate)
}

fn clearable(
    value: Option<String>,
    validate: impl FnOnce(&str) -> Result<(), Status>,
) -> Result<Option<Option<String>>, Status> {
    value
        .map(|value| {
            if value.is_empty() {
                return Ok(None);
            }
            validate(&value)?;
            Ok(Some(value))
        })
        .transpose()
}

fn parse_joined_at(value: Option<Timestamp>) -> Result<Option<DateTime<Utc>>, Status> {
    value
        .map(|value| {
            let joined_at = parse_timestamp(value, "joined_at")?;

            if joined_at > Utc::now() {
                return Err(Status::invalid_argument(
                    "joined_at must not be in the future",
                ));
            }

            Ok(joined_at)
        })
        .transpose()
}

fn parse_role(role: i32) -> Result<Option<MemberRole>, Status> {
    match ProtoRole::try_from(role) {
        Ok(ProtoRole::Unspecified) => Ok(None),
        Ok(ProtoRole::Board) => Ok(Some(MemberRole::Board)),
        Ok(ProtoRole::Mentee) => Ok(Some(MemberRole::Mentee)),
        Ok(ProtoRole::Mentor) => Ok(Some(MemberRole::Mentor)),
        Err(_) => Err(Status::invalid_argument("role is invalid")),
    }
}

impl TryFrom<CreateMemberRequest> for CreateMember {
    type Error = Status;

    fn try_from(input: CreateMemberRequest) -> Result<Self, Status> {
        required(&input.email, "email")?;
        validate_email(&input.email)?;

        validate_optional(&input.discord_id, validate_discord_id)?;
        validate_optional(&input.discord_username, |name| {
            required(name, "discord_username")
        })?;
        if input.discord_id.is_some() != input.discord_username.is_some() {
            return Err(Status::invalid_argument(
                "discord_id and discord_username must be set or cleared together",
            ));
        }

        required(&input.first_name, "first_name")?;
        required(&input.last_name, "last_name")?;

        validate_optional(&input.linkedin_username, |value| {
            validate_username(value, "linkedin_username")
        })?;
        validate_optional(&input.instagram_username, |value| {
            validate_username(value, "instagram_username")
        })?;
        validate_optional(&input.career, |value| required(value, "career"))?;
        validate_optional(&input.industry, |value| required(value, "industry"))?;
        validate_optional(&input.profile, |value| required(value, "profile"))?;

        Ok(Self {
            joined_at: parse_joined_at(input.joined_at)?,
            email: input.email,
            first_name: input.first_name,
            last_name: input.last_name,
            discord_id: input.discord_id,
            discord_username: input.discord_username,
            linkedin_username: input.linkedin_username,
            instagram_username: input.instagram_username,
            career: input.career,
            industry: input.industry,
            profile: input.profile,
            role: input.role.map(parse_role).transpose()?.flatten(),
        })
    }
}

impl TryFrom<UpdateMemberRequest> for UpdateMember {
    type Error = Status;

    fn try_from(input: UpdateMemberRequest) -> Result<Self, Status> {
        validate_member_id(input.member_id)?;

        let discord_id = clearable(input.discord_id, validate_discord_id)?;
        let discord_username = clearable(input.discord_username, |name| {
            required(name, "discord_username")
        })?;

        if let Some(value) = &input.email {
            required(value, "email")?;
            validate_email(value)?;
        }

        if let Some(value) = &input.first_name {
            required(value, "first_name")?;
        }
        if let Some(value) = &input.last_name {
            required(value, "last_name")?;
        }

        let linkedin_username = clearable(input.linkedin_username, |value| {
            validate_username(value, "linkedin_username")
        })?;
        let instagram_username = clearable(input.instagram_username, |value| {
            validate_username(value, "instagram_username")
        })?;
        let career = clearable(input.career, |value| required(value, "career"))?;
        let industry = clearable(input.industry, |value| required(value, "industry"))?;
        let profile = clearable(input.profile, |value| required(value, "profile"))?;
        let role = input.role.map(parse_role).transpose()?;

        Ok(Self {
            member_id: input.member_id,
            joined_at: parse_joined_at(input.joined_at)?,
            email: input.email,
            first_name: input.first_name,
            last_name: input.last_name,
            discord_id,
            discord_username,
            linkedin_username,
            instagram_username,
            career,
            industry,
            profile,
            role,
        })
    }
}

#[tonic::async_trait]
impl<R: MembersRepository> members_admin_service_server::MembersAdminService
    for MembersAdminService<R>
{
    async fn get_sensitive_member(
        &self,
        request: Request<GetMemberRequest>,
    ) -> Result<Response<GetSensitiveMemberResponse>, Status> {
        let member = self.reader.get(request.into_inner()).await?;

        Ok(GetSensitiveMemberResponse {
            member: Some(member),
        }
        .into())
    }

    async fn list_sensitive_members(
        &self,
        request: Request<ListMembersRequest>,
    ) -> Result<Response<ListSensitiveMembersResponse>, Status> {
        let (members, next_page_token) = self.reader.list(request.into_inner()).await?;

        Ok(ListSensitiveMembersResponse {
            members,
            next_page_token,
        }
        .into())
    }

    async fn create_member(
        &self,
        request: Request<CreateMemberRequest>,
    ) -> Result<Response<CreateMemberResponse>, Status> {
        let created_member = self
            .repository
            .create_member(request.into_inner().try_into()?)
            .await
            .map_err(database_status)?;

        Ok(CreateMemberResponse {
            member: Some(created_member.into()),
        }
        .into())
    }

    async fn update_member(
        &self,
        request: Request<UpdateMemberRequest>,
    ) -> Result<Response<UpdateMemberResponse>, Status> {
        let updated_member = self
            .repository
            .update_member(request.into_inner().try_into()?)
            .await
            .map_err(database_status)?;

        Ok(UpdateMemberResponse {
            member: Some(updated_member.into()),
        }
        .into())
    }

    async fn archive_member(
        &self,
        request: Request<ArchiveMemberRequest>,
    ) -> Result<Response<ArchiveMemberResponse>, Status> {
        let input = request.into_inner();
        validate_member_id(input.member_id)?;

        let archived_member = self
            .repository
            .archive_member(input.member_id)
            .await
            .map_err(database_status)?;

        Ok(ArchiveMemberResponse {
            member: Some(archived_member.into()),
        }
        .into())
    }

    async fn bulk_member_operations(
        &self,
        request: Request<BulkMemberOperationsRequest>,
    ) -> Result<Response<BulkMemberOperationsResponse>, Status> {
        let operations = request.into_inner().operations;

        if operations.len() > self.max_bulk_operations {
            return Err(Status::invalid_argument(format!(
                "too many operations: got {}, maximum is {} per request",
                operations.len(),
                self.max_bulk_operations
            )));
        }

        let mut results = Vec::with_capacity(operations.len());
        for (index, operation) in operations.into_iter().enumerate() {
            let kind = match &operation.operation {
                Some(Operation::Create(_)) => "create",
                Some(Operation::Update(_)) => "update",
                Some(Operation::Archive(_)) => "archive",
                Some(Operation::Upsert(_)) => "upsert",
                None => "missing",
            };

            let outcome =
                tokio::time::timeout(self.operation_timeout, self.apply_operation(operation))
                    .await
                    .unwrap_or_else(|_| {
                        tracing::error!(
                            index,
                            kind,
                            timeout = ?self.operation_timeout,
                            "bulk member operation timed out"
                        );
                        Err(Status::deadline_exceeded("bulk member operation timed out"))
                    })
                    .unwrap_or_else(|status| {
                        Outcome::Failed(MemberOperationFailed {
                            error: format!("{:?}: {}", status.code(), status.message()),
                        })
                    });

            results.push(MemberOperationResult {
                index: index as u32,
                outcome: Some(outcome),
            });
        }

        Ok(BulkMemberOperationsResponse { results }.into())
    }
}

fn changed_fields(before: &StoredMember, after: &StoredMember) -> prost_types::FieldMask {
    let fields = [
        ("email", before.email != after.email),
        ("first_name", before.first_name != after.first_name),
        ("last_name", before.last_name != after.last_name),
        ("joined_at", before.joined_at != after.joined_at),
        ("discord_id", before.discord_id != after.discord_id),
        (
            "discord_username",
            before.discord_username != after.discord_username,
        ),
        ("updated_at", before.updated_at != after.updated_at),
        ("archived_at", before.archived_at != after.archived_at),
        (
            "linkedin_username",
            before.linkedin_username != after.linkedin_username,
        ),
        (
            "instagram_username",
            before.instagram_username != after.instagram_username,
        ),
        ("career", before.career != after.career),
        ("industry", before.industry != after.industry),
        ("profile", before.profile != after.profile),
        ("role", before.role != after.role),
    ];

    prost_types::FieldMask {
        paths: fields
            .into_iter()
            .filter(|(_, changed)| *changed)
            .map(|(field, _)| field.to_owned())
            .collect(),
    }
}

impl From<OperationOutcome> for Outcome {
    fn from(value: OperationOutcome) -> Self {
        match value {
            OperationOutcome::Created(member) => Outcome::Created(MemberCreated {
                member: Some(member.into()),
            }),
            OperationOutcome::Archived(member) => Outcome::Archived(MemberArchived {
                member: Some(member.into()),
            }),
            OperationOutcome::Unchanged(member) => Outcome::Unchanged(MemberUnchanged {
                member: Some(member.into()),
            }),
            OperationOutcome::Updated { before, after } => {
                Outcome::Updated(Box::new(MemberUpdated {
                    changed_fields: Some(changed_fields(&before, &after)),
                    before: Some((*before).into()),
                    after: Some((*after).into()),
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::db::members::fake::FakeMembersRepository;
    use tonic::Code;

    use crate::{grpc::timestamp, proto::members::admin::UpsertMemberRequest};

    use super::{members_admin_service_server::MembersAdminService as _, *};

    fn disconnected_service() -> MembersAdminService<FakeMembersRepository> {
        MembersAdminService::new(
            FakeMembersRepository::Unavailable,
            10_000,
            Duration::from_secs(5),
        )
    }

    fn input(email: &str) -> CreateMemberRequest {
        CreateMemberRequest {
            email: email.into(),
            first_name: "First".into(),
            last_name: "Last".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn validation_precedes_database_access() {
        let service = disconnected_service();

        for field in 0..3 {
            let mut value = input("a@example.org");
            match field {
                0 => value.email.clear(),
                1 => value.first_name.clear(),
                _ => value.last_name = "  ".into(),
            }

            assert_eq!(
                service
                    .create_member(Request::new(value))
                    .await
                    .unwrap_err()
                    .code(),
                Code::InvalidArgument
            );
        }

        for (joined_at, message) in [
            (
                prost_types::Timestamp {
                    seconds: 0,
                    nanos: -1,
                },
                "joined_at is invalid",
            ),
            (
                timestamp(Utc::now() + chrono::Duration::days(1)),
                "joined_at must not be in the future",
            ),
        ] {
            let error = service
                .create_member(Request::new(CreateMemberRequest {
                    joined_at: Some(joined_at),
                    ..input("a@example.org")
                }))
                .await
                .unwrap_err();

            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), message);
        }
    }

    fn operation(operation: Operation) -> MemberOperation {
        MemberOperation {
            operation: Some(operation),
        }
    }

    #[tokio::test]
    async fn bulk_validation_and_failures() {
        let service = disconnected_service();

        assert!(
            service
                .bulk_member_operations(Request::new(BulkMemberOperationsRequest::default()))
                .await
                .unwrap()
                .into_inner()
                .results
                .is_empty()
        );

        let limited = MembersAdminService::new(
            FakeMembersRepository::Unavailable,
            1,
            Duration::from_secs(5),
        );
        let error = limited
            .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                operations: vec![MemberOperation::default(), MemberOperation::default()],
            }))
            .await
            .unwrap_err();

        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(
            error.message(),
            "too many operations: got 2, maximum is 1 per request"
        );

        let operations = vec![
            MemberOperation::default(),
            operation(Operation::Upsert(UpsertMemberRequest::default())),
            operation(Operation::Create(input(""))),
            operation(Operation::Update(UpdateMemberRequest {
                member_id: 1,
                first_name: Some("  ".into()),
                ..Default::default()
            })),
            operation(Operation::Archive(ArchiveMemberRequest { member_id: 0 })),
            operation(Operation::Upsert(UpsertMemberRequest {
                member: Some(CreateMemberRequest {
                    joined_at: Some(prost_types::Timestamp {
                        seconds: 0,
                        nanos: -1,
                    }),
                    ..input("valid@example.org")
                }),
            })),
            operation(Operation::Create(input("valid@example.org"))),
        ];

        let results = service
            .bulk_member_operations(Request::new(BulkMemberOperationsRequest { operations }))
            .await
            .unwrap()
            .into_inner()
            .results;

        assert_eq!(results.len(), 7);

        for (index, result) in results.into_iter().enumerate() {
            assert_eq!(result.index, index as u32);

            let Some(Outcome::Failed(failure)) = result.outcome else {
                panic!("expected per-operation failure")
            };

            let expected = [
                "operation is required",
                "upsert member is required",
                "email is required",
                "first_name is required",
                "member_id must be a positive integer",
                "joined_at is invalid",
                "database unavailable",
            ][index];

            assert!(failure.error.contains(expected), "{}", failure.error);
        }
    }

    #[tokio::test]
    async fn bulk_operations_time_out_individually_and_continue() {
        let service = MembersAdminService::new(
            FakeMembersRepository::Hang,
            10_000,
            Duration::from_millis(100),
        );

        let results = service
            .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                operations: vec![
                    operation(Operation::Create(input("a@example.org"))),
                    MemberOperation::default(),
                    operation(Operation::Create(input("b@example.org"))),
                ],
            }))
            .await
            .unwrap()
            .into_inner()
            .results;

        let errors: Vec<_> = results
            .into_iter()
            .map(|result| match result.outcome {
                Some(Outcome::Failed(failure)) => failure.error,
                other => panic!("expected failure, got {other:?}"),
            })
            .collect();

        assert_eq!(
            errors,
            [
                "DeadlineExceeded: bulk member operation timed out",
                "InvalidArgument: operation is required",
                "DeadlineExceeded: bulk member operation timed out",
            ]
        );
    }

    #[tokio::test]
    async fn write_identifiers_follow_lookup_validation() {
        let service = disconnected_service();
        let discord_error =
            "discord_id must be a positive unsigned 64-bit integer containing only digits";

        for (email, discord_id, message) in [
            ("invalid", None, "email must be a valid email address"),
            ("member@", None, "email must be a valid email address"),
            ("a@example.org", Some("abc"), discord_error),
            ("a@example.org", Some("-1"), discord_error),
            ("a@example.org", Some("+1"), discord_error),
            ("a@example.org", Some("1.5"), discord_error),
            ("a@example.org", Some("0"), discord_error),
            ("a@example.org", Some("18446744073709551616"), discord_error),
        ] {
            let create = CreateMemberRequest {
                discord_id: discord_id.map(str::to_owned),
                ..input(email)
            };
            let update = UpdateMemberRequest {
                member_id: 1,
                email: Some(email.into()),
                discord_id: create.discord_id.clone(),
                ..Default::default()
            };

            for error in [
                service
                    .create_member(Request::new(create.clone()))
                    .await
                    .unwrap_err(),
                service
                    .update_member(Request::new(update.clone()))
                    .await
                    .unwrap_err(),
            ] {
                assert_eq!(error.code(), Code::InvalidArgument);
                assert_eq!(error.message(), message);
            }

            let results = service
                .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                    operations: vec![
                        operation(Operation::Create(create.clone())),
                        operation(Operation::Update(update)),
                        operation(Operation::Upsert(UpsertMemberRequest {
                            member: Some(create),
                        })),
                    ],
                }))
                .await
                .unwrap()
                .into_inner()
                .results;

            for result in results {
                let Some(Outcome::Failed(error)) = result.outcome else {
                    panic!("expected validation failure");
                };
                assert_eq!(
                    error.error,
                    format!("{:?}: {message}", Code::InvalidArgument)
                );
            }
        }

        for discord_id in [None, Some("1"), Some("18446744073709551615")] {
            let create = CreateMemberRequest {
                discord_id: discord_id.map(str::to_owned),
                discord_username: discord_id.map(|_| "name".to_owned()),
                ..input("member+tag@example.org")
            };

            assert_eq!(
                service
                    .create_member(Request::new(create.clone()))
                    .await
                    .unwrap_err()
                    .code(),
                Code::Unavailable
            );

            assert_eq!(
                service
                    .update_member(Request::new(UpdateMemberRequest {
                        member_id: 1,
                        email: Some(create.email),
                        discord_id: create.discord_id,
                        discord_username: create.discord_username,
                        ..Default::default()
                    }))
                    .await
                    .unwrap_err()
                    .code(),
                Code::Unavailable
            );
        }

        // "" is not a valid ID on create, but on update it unlinks Discord.
        let error = service
            .create_member(Request::new(CreateMemberRequest {
                discord_id: Some(String::new()),
                ..input("a@example.org")
            }))
            .await
            .unwrap_err();

        assert_eq!(error.message(), discord_error);

        let error = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: 1,
                discord_id: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);

        for (discord_id, discord_username, message) in [
            (
                Some("1"),
                None,
                "discord_id and discord_username must be set or cleared together",
            ),
            (
                None,
                Some("name"),
                "discord_id and discord_username must be set or cleared together",
            ),
            (Some("1"), Some("  "), "discord_username is required"),
        ] {
            let error = service
                .create_member(Request::new(CreateMemberRequest {
                    discord_id: discord_id.map(str::to_owned),
                    discord_username: discord_username.map(str::to_owned),
                    ..input("a@example.org")
                }))
                .await
                .unwrap_err();
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), message);
        }
    }

    #[test]
    fn rejects_blank_update_fields() {
        for field in ["email", "first_name", "last_name"] {
            let mut input = UpdateMemberRequest {
                member_id: 1,
                ..Default::default()
            };

            match field {
                "email" => input.email = Some("  ".into()),
                "first_name" => input.first_name = Some("  ".into()),
                _ => input.last_name = Some("  ".into()),
            }

            let Err(error) = UpdateMember::try_from(input) else {
                panic!("expected blank {field} to be rejected");
            };

            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), format!("{field} is required"));
        }
    }

    #[test]
    fn role_can_be_omitted_set_or_cleared() {
        use crate::domain::members::MemberRole;
        use crate::proto::members::MemberRole as ProtoRole;

        let created = CreateMember::try_from(input("a@example.org")).unwrap();
        assert_eq!(created.role, None);

        let created = CreateMember::try_from(CreateMemberRequest {
            role: Some(ProtoRole::Mentee as i32),
            ..input("a@example.org")
        })
        .unwrap();
        assert_eq!(created.role, Some(MemberRole::Mentee));

        let created = CreateMember::try_from(CreateMemberRequest {
            role: Some(ProtoRole::Unspecified as i32),
            ..input("a@example.org")
        })
        .unwrap();
        assert_eq!(created.role, None);

        let Err(error) = CreateMember::try_from(CreateMemberRequest {
            role: Some(99),
            ..input("a@example.org")
        }) else {
            panic!("expected an invalid role to be rejected");
        };
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), "role is invalid");

        let unchanged = UpdateMember::try_from(UpdateMemberRequest {
            member_id: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(unchanged.role, None);

        let cleared = UpdateMember::try_from(UpdateMemberRequest {
            member_id: 1,
            role: Some(ProtoRole::Unspecified as i32),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cleared.role, Some(None));

        let set = UpdateMember::try_from(UpdateMemberRequest {
            member_id: 1,
            role: Some(ProtoRole::Board as i32),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(set.role, Some(Some(MemberRole::Board)));
    }

    #[test]
    fn roles_survive_writes_and_both_response_views() {
        for (proto, role) in [
            (ProtoRole::Board, MemberRole::Board),
            (ProtoRole::Mentee, MemberRole::Mentee),
            (ProtoRole::Mentor, MemberRole::Mentor),
        ] {
            let created = CreateMember::try_from(CreateMemberRequest {
                role: Some(proto.into()),
                ..input("a@example.org")
            })
            .unwrap();
            assert_eq!(created.role, Some(role));

            let UpsertWrite::Update(update) = UpsertWrite::new(created, Some(1)) else {
                panic!("expected an update for an existing member");
            };
            assert_eq!(update.role, Some(Some(role)));

            let before = stored(1);
            let after = UpdateMember::try_from(UpdateMemberRequest {
                member_id: 1,
                role: Some(proto.into()),
                ..Default::default()
            })
            .unwrap()
            .apply(&before)
            .unwrap();
            assert_eq!(after.role, Some(role));
            assert_eq!(changed_fields(&before, &after).paths, ["role"]);
            let public = crate::proto::members::Member::from(after.clone());
            let sensitive = SensitiveMember::from(after.clone());
            assert_eq!(public.role, Some(proto.into()));
            assert_eq!(sensitive.role, public.role);

            for (requested, expected) in [(None, Some(role)), (Some(0), None)] {
                let updated = UpdateMember::try_from(UpdateMemberRequest {
                    member_id: 1,
                    role: requested,
                    ..Default::default()
                })
                .unwrap()
                .apply(&after)
                .unwrap();
                assert_eq!(updated.role, expected);
            }
        }
    }

    #[test]
    fn updates_reject_unknown_roles() {
        let Err(error) = UpdateMember::try_from(UpdateMemberRequest {
            member_id: 1,
            role: Some(99),
            ..Default::default()
        }) else {
            panic!("expected an invalid role to be rejected");
        };
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), "role is invalid");
    }

    #[test]
    fn changed_fields_lists_only_differing_fields() {
        let before = StoredMember {
            discord_id: Some("1".into()),
            discord_username: Some("name".into()),
            profile: Some("notes".into()),
            ..stored(1)
        };

        assert!(changed_fields(&before, &before).paths.is_empty());

        let after = StoredMember {
            email: "b@example.org".into(),
            discord_id: None,
            discord_username: None,
            career: Some("Designer".into()),
            profile: None,
            ..before.clone()
        };

        assert_eq!(
            changed_fields(&before, &after).paths,
            [
                "email",
                "discord_id",
                "discord_username",
                "career",
                "profile"
            ]
        );
    }

    #[tokio::test]
    async fn optional_profile_fields_are_validated_on_every_write_path() {
        let service = disconnected_service();
        let url_error = "linkedin_username must be a bare username, not a URL or @handle";
        let handle_error = "instagram_username must be a bare username, not a URL or @handle";

        for (create, message) in [
            (
                CreateMemberRequest {
                    linkedin_username: Some("https://www.linkedin.com/in/xyz".into()),
                    ..input("a@example.org")
                },
                url_error,
            ),
            (
                CreateMemberRequest {
                    linkedin_username: Some("in/xyz".into()),
                    ..input("a@example.org")
                },
                url_error,
            ),
            (
                CreateMemberRequest {
                    instagram_username: Some("@xyz".into()),
                    ..input("a@example.org")
                },
                handle_error,
            ),
            (
                CreateMemberRequest {
                    instagram_username: Some("x yz".into()),
                    ..input("a@example.org")
                },
                handle_error,
            ),
            (
                CreateMemberRequest {
                    career: Some(" ".into()),
                    ..input("a@example.org")
                },
                "career is required",
            ),
            (
                CreateMemberRequest {
                    industry: Some(String::new()),
                    ..input("a@example.org")
                },
                "industry is required",
            ),
            (
                CreateMemberRequest {
                    profile: Some("\n".into()),
                    ..input("a@example.org")
                },
                "profile is required",
            ),
        ] {
            let update = UpdateMemberRequest {
                member_id: 1,
                linkedin_username: create.linkedin_username.clone(),
                instagram_username: create.instagram_username.clone(),
                career: create.career.clone(),
                industry: create.industry.clone().filter(|value| !value.is_empty()),
                profile: create.profile.clone(),
                ..Default::default()
            };

            let mut errors = vec![
                service
                    .create_member(Request::new(create.clone()))
                    .await
                    .unwrap_err(),
            ];
            if update
                != (UpdateMemberRequest {
                    member_id: 1,
                    ..Default::default()
                })
            {
                errors.push(
                    service
                        .update_member(Request::new(update))
                        .await
                        .unwrap_err(),
                );
            }

            let results = service
                .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                    operations: vec![operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(create),
                    }))],
                }))
                .await
                .unwrap()
                .into_inner()
                .results;
            let Some(Outcome::Failed(failure)) = &results[0].outcome else {
                panic!("expected upsert validation failure");
            };
            assert_eq!(failure.error, format!("InvalidArgument: {message}"));

            for error in errors {
                assert_eq!(error.code(), Code::InvalidArgument);
                assert_eq!(error.message(), message);
            }
        }

        let error = service
            .create_member(Request::new(CreateMemberRequest {
                linkedin_username: Some("xyz-123".into()),
                instagram_username: Some("x.y_z".into()),
                career: Some("Software Engineer".into()),
                industry: Some("Technology".into()),
                profile: Some("notes".into()),
                ..input("a@example.org")
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::Unavailable);
    }

    fn stored(id: i32) -> StoredMember {
        StoredMember {
            id,
            email: format!("m{id}@example.org"),
            first_name: "First".into(),
            last_name: "Last".into(),
            joined_at: DateTime::UNIX_EPOCH,
            discord_id: None,
            discord_username: None,
            linkedin_username: Some(format!("member-{id}")),
            instagram_username: None,
            career: Some("Engineer".into()),
            industry: None,
            profile: Some(format!("notes {id}")),
            role: None,
            updated_at: DateTime::UNIX_EPOCH,
            archived_at: (id == 2).then_some(DateTime::UNIX_EPOCH),
        }
    }

    fn stored_service() -> MembersAdminService<FakeMembersRepository> {
        MembersAdminService::new(
            FakeMembersRepository::Stored(vec![stored(1), stored(2)]),
            10_000,
            Duration::from_secs(5),
        )
    }

    #[tokio::test]
    async fn sensitive_reads_include_admin_only_fields() {
        let service = stored_service();

        let member = service
            .get_sensitive_member(Request::new(GetMemberRequest {
                identifier: Some(
                    crate::proto::members::get_member_request::Identifier::Email(
                        "m1@example.org".into(),
                    ),
                ),
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert_eq!(member.profile.as_deref(), Some("notes 1"));
        assert_eq!(member.linkedin_username.as_deref(), Some("member-1"));

        let error = service
            .get_sensitive_member(Request::new(GetMemberRequest { identifier: None }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);

        let page = service
            .list_sensitive_members(Request::new(ListMembersRequest {
                include_archived: true,
                page_size: 1,
                page_token: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(page.next_page_token, "1:1");
        let profiles: Vec<_> = page
            .members
            .iter()
            .map(|member| member.profile.as_deref())
            .collect();
        assert_eq!(profiles, [Some("notes 1")]);
    }

    #[tokio::test]
    async fn single_writes_return_the_stored_member() {
        let service = stored_service();
        let joined_at = Timestamp {
            seconds: 1_000,
            nanos: 0,
        };

        let created = service
            .create_member(Request::new(CreateMemberRequest {
                joined_at: Some(joined_at),
                profile: Some("new notes".into()),
                ..input("new@example.org")
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert_eq!(created.profile.as_deref(), Some("new notes"));
        assert_eq!(created.member_id, 3);
        assert_eq!(created.joined_at, Some(joined_at));

        let updated = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: 1,
                first_name: Some("New".into()),
                linkedin_username: Some(String::new()),
                industry: Some("Finance".into()),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert_eq!(updated.profile.as_deref(), Some("notes 1"));
        assert_eq!(updated.first_name, "New");
        assert_eq!(updated.last_name, "Last");
        assert_eq!(updated.linkedin_username, None);
        assert_eq!(updated.career.as_deref(), Some("Engineer"));
        assert_eq!(updated.industry.as_deref(), Some("Finance"));

        let archived = service
            .archive_member(Request::new(ArchiveMemberRequest { member_id: 1 }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert!(archived.archived_at.is_some());

        let error = service
            .archive_member(Request::new(ArchiveMemberRequest { member_id: 0 }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn bulk_operations_report_each_outcome() {
        let changed = |email: &str| CreateMemberRequest {
            first_name: "Changed".into(),
            ..input(email)
        };

        let results = stored_service()
            .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                operations: vec![
                    operation(Operation::Create(input("new@example.org"))),
                    operation(Operation::Update(UpdateMemberRequest {
                        member_id: 1,
                        first_name: Some("New".into()),
                        ..Default::default()
                    })),
                    operation(Operation::Update(UpdateMemberRequest {
                        member_id: 1,
                        first_name: Some("First".into()),
                        ..Default::default()
                    })),
                    operation(Operation::Archive(ArchiveMemberRequest { member_id: 1 })),
                    operation(Operation::Archive(ArchiveMemberRequest { member_id: 2 })),
                    operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(changed("m1@example.org")),
                    })),
                    operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(changed("fresh@example.org")),
                    })),
                    operation(Operation::Update(UpdateMemberRequest {
                        member_id: 1,
                        profile: Some("rewritten".into()),
                        ..Default::default()
                    })),
                ],
            }))
            .await
            .unwrap()
            .into_inner()
            .results;

        let outcomes: Vec<_> = results
            .into_iter()
            .map(|result| match result.outcome {
                Some(Outcome::Created(created)) => ("created", created.member.unwrap().first_name),
                Some(Outcome::Updated(updated)) => {
                    ("updated", updated.changed_fields.unwrap().paths.join(","))
                }
                Some(Outcome::Unchanged(unchanged)) => {
                    ("unchanged", unchanged.member.unwrap().first_name)
                }
                Some(Outcome::Archived(archived)) => ("archived", archived.member.unwrap().email),
                other => panic!("unexpected outcome {other:?}"),
            })
            .collect();

        assert_eq!(
            outcomes,
            [
                ("created", "First".to_owned()),
                ("updated", "first_name".to_owned()),
                ("unchanged", "First".to_owned()),
                ("archived", "m1@example.org".to_owned()),
                ("unchanged", "First".to_owned()),
                ("updated", "first_name".to_owned()),
                ("created", "Changed".to_owned()),
                ("updated", "profile".to_owned()),
            ]
        );
    }
}
