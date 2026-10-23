use std::time::Duration;

use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::{
    db::members::MembersRepository,
    domain::members::{
        CreateMember, MemberIdentifier, OperationOutcome, UpdateMember, UpsertWrite,
    },
    grpc::{
        database_status, parse_timestamp, validate_discord_id, validate_email, validate_member_id,
    },
    proto::members::admin::{
        ArchiveMemberRequest, ArchiveMemberResponse, BulkMemberOperationsRequest,
        BulkMemberOperationsResponse, CreateMemberRequest, CreateMemberResponse, MemberArchived,
        MemberCreated, MemberOperation, MemberOperationFailed, MemberOperationResult,
        MemberUnchanged, MemberUpdated, UpdateMemberRequest, UpdateMemberResponse,
        member_operation::Operation, member_operation_result::Outcome,
        members_admin_service_server,
    },
};

pub use members_admin_service_server::MembersAdminServiceServer;

pub struct MembersAdminService<R> {
    repository: R,
    max_bulk_operations: usize,
    operation_timeout: Duration,
}

impl<R: MembersRepository> MembersAdminService<R> {
    pub fn new(repository: R, max_bulk_operations: usize, operation_timeout: Duration) -> Self {
        Self {
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

impl TryFrom<CreateMemberRequest> for CreateMember {
    type Error = Status;

    fn try_from(input: CreateMemberRequest) -> Result<Self, Status> {
        required(&input.email, "email")?;
        validate_email(&input.email)?;

        if let Some(id) = &input.discord_id {
            validate_discord_id(id)?;
        }
        if let Some(name) = &input.discord_username {
            required(name, "discord_username")?;
        }
        if input.discord_id.is_some() != input.discord_username.is_some() {
            return Err(Status::invalid_argument(
                "discord_id and discord_username must be set or cleared together",
            ));
        }

        required(&input.first_name, "first_name")?;
        required(&input.last_name, "last_name")?;

        Ok(Self {
            joined_at: parse_joined_at(input.joined_at)?,
            email: input.email,
            first_name: input.first_name,
            last_name: input.last_name,
            discord_id: input.discord_id,
            discord_username: input.discord_username,
        })
    }
}

impl TryFrom<UpdateMemberRequest> for UpdateMember {
    type Error = Status;

    fn try_from(input: UpdateMemberRequest) -> Result<Self, Status> {
        validate_member_id(input.member_id)?;

        // An explicit "" clears the Discord field; anything else must be valid.
        if let Some(id) = input.discord_id.as_deref().filter(|id| !id.is_empty()) {
            validate_discord_id(id)?;
        }
        if let Some(name) = input
            .discord_username
            .as_deref()
            .filter(|name| !name.is_empty())
        {
            required(name, "discord_username")?;
        }

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

        Ok(Self {
            member_id: input.member_id,
            joined_at: parse_joined_at(input.joined_at)?,
            email: input.email,
            first_name: input.first_name,
            last_name: input.last_name,
            discord_id: input.discord_id.map(|id| (!id.is_empty()).then_some(id)),
            discord_username: input
                .discord_username
                .map(|name| (!name.is_empty()).then_some(name)),
        })
    }
}

#[tonic::async_trait]
impl<R: MembersRepository> members_admin_service_server::MembersAdminService
    for MembersAdminService<R>
{
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

fn changed_fields(
    before: &crate::proto::members::Member,
    after: &crate::proto::members::Member,
) -> prost_types::FieldMask {
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
                let before = before.into();
                let after = after.into();

                Outcome::Updated(Box::new(MemberUpdated {
                    changed_fields: Some(changed_fields(&before, &after)),
                    before: Some(before),
                    after: Some(after),
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

        let limited =
            MembersAdminService::new(service.repository.clone(), 1, Duration::from_secs(5));
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
    fn changed_fields_lists_only_differing_fields() {
        let before = crate::proto::members::Member {
            email: "a@example.org".into(),
            discord_id: Some("1".into()),
            discord_username: Some("name".into()),
            ..Default::default()
        };

        assert!(changed_fields(&before, &before).paths.is_empty());

        let after = crate::proto::members::Member {
            email: "b@example.org".into(),
            discord_id: None,
            discord_username: None,
            ..before.clone()
        };

        assert_eq!(
            changed_fields(&before, &after).paths,
            ["email", "discord_id", "discord_username"]
        );
    }

    fn stored(id: i32) -> crate::domain::members::Member {
        crate::domain::members::Member {
            id,
            email: format!("m{id}@example.org"),
            first_name: "First".into(),
            last_name: "Last".into(),
            joined_at: DateTime::UNIX_EPOCH,
            discord_id: None,
            discord_username: None,
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
    async fn single_writes_return_the_stored_member() {
        let service = stored_service();
        let joined_at = Timestamp {
            seconds: 1_000,
            nanos: 0,
        };

        let created = service
            .create_member(Request::new(CreateMemberRequest {
                joined_at: Some(joined_at),
                ..input("new@example.org")
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert_eq!(created.member_id, 3);
        assert_eq!(created.joined_at, Some(joined_at));

        let updated = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: 1,
                first_name: Some("New".into()),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert_eq!(updated.first_name, "New");
        assert_eq!(updated.last_name, "Last");

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
            ]
        );
    }
}
