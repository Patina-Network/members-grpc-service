use tonic::{Request, Response, Status};

use crate::proto::members::{
    GetMemberRequest, GetMemberResponse, ListMembersRequest, ListMembersResponse,
    members_service_server,
};

// main will need this
pub use members_service_server::MembersServiceServer;

#[derive(Debug, Default)]
pub struct MembersService;

#[tonic::async_trait]
impl members_service_server::MembersService for MembersService {
    async fn get_member(
        &self,
        _request: Request<GetMemberRequest>,
    ) -> Result<Response<GetMemberResponse>, Status> {
        unimplemented!("members.MembersService/getMember")
    }

    async fn list_members(
        &self,
        _request: Request<ListMembersRequest>,
    ) -> Result<Response<ListMembersResponse>, Status> {
        unimplemented!("members.MembersService/listMembers")
    }
}
