use tonic::{Request, Response, Status};

use crate::{
    db::members_admin::MembersAdminRepository,
    proto::members::admin::{
        ArchiveMemberRequest, ArchiveMemberResponse, BulkMemberOperationsRequest,
        BulkMemberOperationsResponse, CreateMemberRequest, CreateMemberResponse,
        UpdateMemberRequest, UpdateMemberResponse, members_admin_service_server,
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
            // PostgreSQL stores timestamps at microsecond precision.
            Ok(v.seconds * 1_000_000 + i64::from(v.nanos / 1000))
        })
        .transpose()
}

fn db_error(error: sqlx::Error) -> Status {
    match &error {
        sqlx::Error::RowNotFound => return Status::not_found("member not found"),
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            return Status::already_exists("email or discord_id already exists");
        }
        sqlx::Error::Database(db) if db.code().as_deref() == Some("22P02") => {
            return Status::invalid_argument("member_id must be a UUID");
        }
        sqlx::Error::PoolClosed
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => {
            tracing::error!(%error, "member storage unavailable");
            return Status::unavailable("member storage unavailable");
        }
        _ => {}
    }
    tracing::error!(%error, "member write failed");
    Status::internal("member write failed")
}

#[tonic::async_trait]
impl members_admin_service_server::MembersAdminService for MembersAdminService {
    async fn create_member(
        &self,
        request: Request<CreateMemberRequest>,
    ) -> Result<Response<CreateMemberResponse>, Status> {
        authorize(&request)?;
        let input = request.into_inner();
        required(&input.email, "email")?;
        required(&input.first_name, "first_name")?;
        required(&input.last_name, "last_name")?;
        let joined = timestamp(input.joined_at.as_ref())?;
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
        required(&input.member_id, "member_id")?;
        if let Some(value) = &input.email {
            required(value, "email")?;
        }
        if let Some(value) = &input.first_name {
            required(value, "first_name")?;
        }
        if let Some(value) = &input.last_name {
            required(value, "last_name")?;
        }
        let joined = timestamp(input.joined_at.as_ref())?;
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
        required(&input.member_id, "member_id")?;
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
        _request: Request<BulkMemberOperationsRequest>,
    ) -> Result<Response<BulkMemberOperationsResponse>, Status> {
        unimplemented!("members.admin.MembersAdminService/bulkMemberOperations")
    }
}

#[cfg(test)]
mod tests;
