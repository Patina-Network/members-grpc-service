use tonic::{Request, Response, Status};

use crate::proto::members::admin::{
    ArchiveMemberRequest, ArchiveMemberResponse, BulkMemberOperationsRequest,
    BulkMemberOperationsResponse, CreateMemberRequest, CreateMemberResponse, UpdateMemberRequest,
    UpdateMemberResponse, members_admin_service_server,
};

// main will need this
pub use members_admin_service_server::MembersAdminServiceServer;

#[derive(Debug, Default)]
pub struct MembersAdminService;

#[tonic::async_trait]
impl members_admin_service_server::MembersAdminService for MembersAdminService {
    async fn create_member(
        &self,
        _request: Request<CreateMemberRequest>,
    ) -> Result<Response<CreateMemberResponse>, Status> {
        unimplemented!("members.admin.MembersAdminService/createMember")
    }

    async fn update_member(
        &self,
        _request: Request<UpdateMemberRequest>,
    ) -> Result<Response<UpdateMemberResponse>, Status> {
        unimplemented!("members.admin.MembersAdminService/updateMember")
    }

    async fn archive_member(
        &self,
        _request: Request<ArchiveMemberRequest>,
    ) -> Result<Response<ArchiveMemberResponse>, Status> {
        unimplemented!("members.admin.MembersAdminService/archiveMember")
    }

    async fn bulk_member_operations(
        &self,
        _request: Request<BulkMemberOperationsRequest>,
    ) -> Result<Response<BulkMemberOperationsResponse>, Status> {
        unimplemented!("members.admin.MembersAdminService/bulkMemberOperations")
    }
}
