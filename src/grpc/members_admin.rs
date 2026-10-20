use prost::Message;
use tonic::{Request, Response, Status};

use super::members::{validate_discord_id, validate_email};

use crate::{
    db::members_admin::{CreateMember, MembersAdminRepository, OperationOutcome, UpdateMember},
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
    repository: MembersAdminRepository,
}

impl MembersAdminService {
    pub fn new(repository: MembersAdminRepository) -> Self {
        Self { repository }
    }

    async fn apply_operation(&self, input: MemberOperation) -> Result<Outcome, Status> {
        match input
            .operation
            .ok_or_else(|| Status::invalid_argument("operation is required"))?
        {
            Operation::Create(input) => {
                let joined = validate_create(&input)?;
                let stored = self
                    .repository
                    .create_member(create_input(input, joined))
                    .await
                    .map_err(db_error)?;
                Ok(Outcome::Created(MemberCreated {
                    member: Some(stored.into()),
                }))
            }
            Operation::Update(input) => {
                let joined = validate_update(&input)?;
                self.repository
                    .update_operation(update_input(input, joined))
                    .await
                    .map(Into::into)
                    .map_err(db_error)
            }
            Operation::Archive(input) => {
                member_id(input.member_id)?;
                self.repository
                    .archive_operation(input.member_id)
                    .await
                    .map(Into::into)
                    .map_err(db_error)
            }
            Operation::Upsert(input) => {
                let input = input
                    .member
                    .ok_or_else(|| Status::invalid_argument("upsert member is required"))?;
                let joined = validate_create(&input)?;
                self.repository
                    .upsert_member(create_input(input, joined))
                    .await
                    .map(Into::into)
                    .map_err(db_error)
            }
        }
    }
}

fn authorize<T>(request: &Request<T>) -> Result<(), Status> {
    let mut profiles = request.metadata().get_all("profile").iter();
    if profiles.next().and_then(|value| value.to_str().ok()) != Some("core_admin")
        || profiles.next().is_some()
    {
        return Err(Status::permission_denied("PROFILE=core_admin is required"));
    }
    Ok(())
}

fn required(value: &str, field: &str) -> Result<(), Status> {
    if value.trim().is_empty() {
        return Err(Status::invalid_argument(format!("{field} is required")));
    }
    Ok(())
}

fn timestamp(value: Option<&prost_types::Timestamp>) -> Result<Option<i64>, Status> {
    value
        .map(|v| {
            if !(-62_135_596_800..=253_402_300_799).contains(&v.seconds)
                || !(0..1_000_000_000).contains(&v.nanos)
            {
                return Err(Status::invalid_argument("joined_at is invalid"));
            }
            Ok(v.seconds * 1_000_000 + i64::from(v.nanos / 1000))
        })
        .transpose()
}

fn db_error(error: sqlx::Error) -> Status {
    match &error {
        sqlx::Error::RowNotFound => Status::not_found("member not found"),
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Status::already_exists("email or discord_id already exists")
        }
        _ => crate::grpc::database_status(error),
    }
}

fn member_id(value: i32) -> Result<(), Status> {
    if value <= 0 {
        return Err(Status::invalid_argument(
            "member_id must be a positive integer",
        ));
    }
    Ok(())
}

fn validate_create(input: &CreateMemberRequest) -> Result<Option<i64>, Status> {
    required(&input.email, "email")?;
    validate_email(&input.email)?;
    if let Some(id) = &input.discord_id {
        validate_discord_id(id)?;
    }
    required(&input.first_name, "first_name")?;
    required(&input.last_name, "last_name")?;
    timestamp(input.joined_at.as_ref())
}

fn validate_update(input: &UpdateMemberRequest) -> Result<Option<i64>, Status> {
    member_id(input.member_id)?;
    if let Some(id) = &input.discord_id {
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
    timestamp(input.joined_at.as_ref())
}

#[tonic::async_trait]
impl members_admin_service_server::MembersAdminService for MembersAdminService {
    async fn create_member(
        &self,
        request: Request<CreateMemberRequest>,
    ) -> Result<Response<CreateMemberResponse>, Status> {
        authorize(&request)?;
        let input = request.into_inner();
        let joined = validate_create(&input)?;
        let stored = self
            .repository
            .create_member(create_input(input, joined))
            .await
            .map_err(db_error)?;
        Ok(CreateMemberResponse {
            member: Some(stored.into()),
        }
        .into())
    }

    async fn update_member(
        &self,
        request: Request<UpdateMemberRequest>,
    ) -> Result<Response<UpdateMemberResponse>, Status> {
        authorize(&request)?;
        let input = request.into_inner();
        let joined = validate_update(&input)?;
        let stored = self
            .repository
            .update_member(update_input(input, joined))
            .await
            .map_err(db_error)?;
        Ok(UpdateMemberResponse {
            member: Some(stored.into()),
        }
        .into())
    }

    async fn archive_member(
        &self,
        request: Request<ArchiveMemberRequest>,
    ) -> Result<Response<ArchiveMemberResponse>, Status> {
        authorize(&request)?;
        let input = request.into_inner();
        member_id(input.member_id)?;
        let stored = self
            .repository
            .archive_member(input.member_id)
            .await
            .map_err(db_error)?;
        Ok(ArchiveMemberResponse {
            member: Some(stored.into()),
        }
        .into())
    }

    async fn bulk_member_operations(
        &self,
        request: Request<BulkMemberOperationsRequest>,
    ) -> Result<Response<BulkMemberOperationsResponse>, Status> {
        authorize(&request)?;
        let operations = request.into_inner().operations;
        u32::try_from(operations.len())
            .map_err(|_| Status::invalid_argument("too many operations"))?;
        let mut results = Vec::with_capacity(operations.len());
        for (index, operation) in operations.into_iter().enumerate() {
            let outcome = self
                .apply_operation(operation)
                .await
                .unwrap_or_else(|status| {
                    Outcome::Failed(MemberOperationFailed {
                        error: format!("{}: {}", status.code(), status.message()),
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

fn create_input(input: CreateMemberRequest, joined_at: Option<i64>) -> CreateMember {
    CreateMember {
        email: input.email,
        first_name: input.first_name,
        last_name: input.last_name,
        joined_at,
        discord_id: input.discord_id,
        discord_username: input.discord_username,
    }
}

fn update_input(input: UpdateMemberRequest, joined_at: Option<i64>) -> UpdateMember {
    UpdateMember {
        member_id: input.member_id,
        email: input.email,
        first_name: input.first_name,
        last_name: input.last_name,
        joined_at,
        discord_id: input.discord_id,
        discord_username: input.discord_username,
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

pub fn encode_event(value: &OperationOutcome) -> (&'static str, Vec<u8>) {
    match Outcome::from(value.clone()) {
        Outcome::Created(event) => ("MemberCreated", event.encode_to_vec()),
        Outcome::Updated(event) => ("MemberUpdated", event.encode_to_vec()),
        Outcome::Archived(event) => ("MemberArchived", event.encode_to_vec()),
        Outcome::Unchanged(event) => ("MemberUnchanged", event.encode_to_vec()),
        Outcome::Failed(event) => ("MemberOperationFailed", event.encode_to_vec()),
    }
}

#[cfg(test)]
mod tests;
