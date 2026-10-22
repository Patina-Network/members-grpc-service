use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::{
    db::members::{
        CreateMember, MemberIdentifier, MembersRepository, OperationOutcome, UpdateMember,
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

pub struct MembersAdminService {
    repository: MembersRepository,
    max_bulk_operations: usize,
}

impl MembersAdminService {
    pub fn new(repository: MembersRepository, max_bulk_operations: usize) -> Self {
        Self {
            repository,
            max_bulk_operations,
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

                let existing_member = self
                    .repository
                    .get(MemberIdentifier::Email(member.email.clone()))
                    .await
                    .map_err(database_status)?;

                let outcome = match existing_member {
                    Some(existing) => {
                        self.repository
                            .update_operation(UpdateMember {
                                member_id: existing.id,
                                email: Some(member.email),
                                first_name: Some(member.first_name),
                                last_name: Some(member.last_name),
                                joined_at: member.joined_at,
                                discord_id: member.discord_id.map(Some),
                                discord_username: member.discord_username,
                            })
                            .await
                    }
                    None => self
                        .repository
                        .create_member(member)
                        .await
                        .map(OperationOutcome::Created),
                };

                outcome.map(Into::into).map_err(database_status)
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

        // An explicit "" unlinks Discord; anything else must be a valid ID.
        if let Some(id) = input.discord_id.as_deref().filter(|id| !id.is_empty()) {
            validate_discord_id(id)?;
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
            discord_username: input.discord_username,
        })
    }
}

#[tonic::async_trait]
impl members_admin_service_server::MembersAdminService for MembersAdminService {
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
            let outcome = self
                .apply_operation(operation)
                .await
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
    use sqlx::postgres::PgPoolOptions;
    use tonic::Code;

    use crate::{grpc::timestamp, proto::members::admin::UpsertMemberRequest};

    use super::{members_admin_service_server::MembersAdminService as _, *};

    async fn disconnected_service() -> MembersAdminService {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/unused")
            .unwrap();
        pool.close().await;

        MembersAdminService::new(MembersRepository::new(pool), 10_000)
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
        let service = disconnected_service().await;

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
        let service = disconnected_service().await;

        assert!(
            service
                .bulk_member_operations(Request::new(BulkMemberOperationsRequest::default()))
                .await
                .unwrap()
                .into_inner()
                .results
                .is_empty()
        );

        let limited = MembersAdminService::new(service.repository.clone(), 1);
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
    async fn write_identifiers_follow_lookup_validation() {
        let service = disconnected_service().await;
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

    #[tokio::test]
    async fn single_and_bulk_writes_preserve_member_state() {
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

        let service = MembersAdminService::new(MembersRepository::new(pool.clone()), 10_000);

        let created = service
            .create_member(Request::new(input("one@example.org")))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();

        assert!(created.member_id > 0);
        assert!(created.joined_at.is_some());

        let error = service
            .create_member(Request::new(input("one@example.org")))
            .await
            .unwrap_err();

        assert_eq!(error.code(), Code::AlreadyExists);
        assert_eq!(error.message(), "email already exists");

        let linked = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: created.member_id,
                discord_id: Some("42".into()),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();

        assert_eq!(linked.discord_id.as_deref(), Some("42"));

        let error = service
            .create_member(Request::new(CreateMemberRequest {
                discord_id: Some("42".into()),
                ..input("two@example.org")
            }))
            .await
            .unwrap_err();

        assert_eq!(error.code(), Code::AlreadyExists);
        assert_eq!(error.message(), "discord_id already exists");

        let kept = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: created.member_id,
                last_name: Some("Kept".into()),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();

        assert_eq!(kept.discord_id.as_deref(), Some("42"));

        let unlinked = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: created.member_id,
                discord_id: Some(String::new()),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();
        assert_eq!(unlinked.discord_id, None);

        let updated = service
            .update_member(Request::new(UpdateMemberRequest {
                member_id: created.member_id,
                first_name: Some("New".into()),
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();

        assert_eq!(updated.first_name, "New");
        assert_eq!(updated.email, created.email);
        assert_eq!(updated.joined_at, created.joined_at);

        let archived = service
            .archive_member(Request::new(ArchiveMemberRequest {
                member_id: created.member_id,
            }))
            .await
            .unwrap()
            .into_inner()
            .member
            .unwrap();

        assert!(archived.archived_at.is_some());
        assert_eq!(archived.archived_at, archived.updated_at);

        assert_eq!(
            service
                .archive_member(Request::new(ArchiveMemberRequest {
                    member_id: created.member_id
                }))
                .await
                .unwrap()
                .into_inner()
                .member,
            Some(archived.clone())
        );

        for error in [
            service
                .update_member(Request::new(UpdateMemberRequest {
                    member_id: i32::MAX,
                    ..Default::default()
                }))
                .await
                .unwrap_err(),
            service
                .archive_member(Request::new(ArchiveMemberRequest {
                    member_id: i32::MAX,
                }))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code(), Code::NotFound);
        }

        let member = input("bulk@example.org");
        let changed = CreateMemberRequest {
            first_name: "Changed".into(),
            ..member.clone()
        };

        let results = service
            .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                operations: vec![
                    operation(Operation::Create(member.clone())),
                    operation(Operation::Create(member)),
                    operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(changed.clone()),
                    })),
                    operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(changed),
                    })),
                    operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(input("new@example.org")),
                    })),
                ],
            }))
            .await
            .unwrap()
            .into_inner()
            .results;

        for (index, result) in results.iter().enumerate() {
            assert_eq!(result.index, index as u32);
        }

        assert!(matches!(results[0].outcome, Some(Outcome::Created(_))));
        assert!(matches!(results[1].outcome, Some(Outcome::Failed(_))));

        let Some(Outcome::Updated(event)) = &results[2].outcome else {
            panic!("expected upsert update");
        };
        assert_eq!(event.after.as_ref().unwrap().first_name, "Changed");

        let Some(Outcome::Unchanged(unchanged)) = &results[3].outcome else {
            panic!("expected unchanged upsert");
        };
        assert_eq!(unchanged.member, event.after);

        assert!(matches!(results[4].outcome, Some(Outcome::Created(_))));

        let bulk_id = event.after.as_ref().unwrap().member_id;
        assert_eq!(
            service
                .update_member(Request::new(UpdateMemberRequest {
                    member_id: bulk_id,
                    email: Some("one@example.org".into()),
                    ..Default::default()
                }))
                .await
                .unwrap_err()
                .code(),
            Code::AlreadyExists
        );

        let results = service
            .bulk_member_operations(Request::new(BulkMemberOperationsRequest {
                operations: vec![
                    operation(Operation::Archive(ArchiveMemberRequest {
                        member_id: bulk_id,
                    })),
                    operation(Operation::Archive(ArchiveMemberRequest {
                        member_id: bulk_id,
                    })),
                    operation(Operation::Update(UpdateMemberRequest {
                        member_id: bulk_id,
                        ..Default::default()
                    })),
                ],
            }))
            .await
            .unwrap()
            .into_inner()
            .results;

        let Some(Outcome::Archived(first)) = &results[0].outcome else {
            panic!("expected archive");
        };

        for result in &results[1..] {
            let Some(Outcome::Unchanged(value)) = &result.outcome else {
                panic!("expected unchanged member");
            };

            assert_eq!(value.member, first.member);
        }

        pool.close().await;
    }
}
