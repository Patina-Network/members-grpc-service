use tonic::{Request, Response, Status};

use crate::{
    db::members_admin::MembersAdminRepository,
    proto::members::admin::{
        ArchiveMemberRequest, ArchiveMemberResponse, BulkMemberOperationsRequest,
        BulkMemberOperationsResponse, CreateMemberRequest, CreateMemberResponse, MemberCreated,
        MemberOperation, MemberOperationFailed, MemberOperationResult, UpdateMemberRequest,
        UpdateMemberResponse, member_operation::Operation, member_operation_result::Outcome,
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
                    .create_member(input, joined)
                    .await
                    .map_err(db_error)?;
                Ok(Outcome::Created(MemberCreated {
                    member: Some(stored),
                }))
            }
            Operation::Update(input) => {
                let joined = validate_update(&input)?;
                self.repository
                    .update_operation(input, joined)
                    .await
                    .map_err(db_error)
            }
            Operation::Archive(input) => {
                member_id(input.member_id)?;
                self.repository
                    .archive_operation(input)
                    .await
                    .map_err(db_error)
            }
            Operation::Upsert(input) => {
                let input = input
                    .member
                    .ok_or_else(|| Status::invalid_argument("upsert member is required"))?;
                let joined = validate_create(&input)?;
                self.repository
                    .upsert_member(input, joined)
                    .await
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
    required(&input.first_name, "first_name")?;
    required(&input.last_name, "last_name")?;
    timestamp(input.joined_at.as_ref())
}

fn validate_update(input: &UpdateMemberRequest) -> Result<Option<i64>, Status> {
    member_id(input.member_id)?;
    if let Some(value) = &input.email {
        required(value, "email")?;
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
            .create_member(input, joined)
            .await
            .map_err(db_error)?;
        Ok(Response::new(CreateMemberResponse {
            member: Some(stored),
        }))
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
            .update_member(input, joined)
            .await
            .map_err(db_error)?;
        Ok(Response::new(UpdateMemberResponse {
            member: Some(stored),
        }))
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
            .archive_member(input)
            .await
            .map_err(db_error)?;
        Ok(Response::new(ArchiveMemberResponse {
            member: Some(stored),
        }))
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
        Ok(Response::new(BulkMemberOperationsResponse { results }))
    }
}

#[cfg(test)]
mod tests;
