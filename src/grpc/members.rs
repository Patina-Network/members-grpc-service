use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use prost_types::Timestamp;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
    db::members::{Member as StoredMember, MemberIdentifier, MembersRepository},
    proto::members::{
        GetMemberRequest, GetMemberResponse, ListMembersRequest, ListMembersResponse, Member,
        get_member_request::Identifier, members_service_server,
    },
};

pub use members_service_server::MembersServiceServer;

const DEFAULT_PAGE_SIZE: i32 = 50;
const MAX_PAGE_SIZE: i32 = 100;

pub struct MembersService {
    repository: MembersRepository,
}

impl MembersService {
    pub fn new(repository: MembersRepository) -> Self {
        Self { repository }
    }
}

#[tonic::async_trait]
impl members_service_server::MembersService for MembersService {
    async fn get_member(
        &self,
        request: Request<GetMemberRequest>,
    ) -> Result<Response<GetMemberResponse>, Status> {
        let identifier = match request.into_inner().identifier {
            Some(Identifier::MemberId(id)) => MemberIdentifier::Id(
                Uuid::parse_str(&id)
                    .map_err(|_| Status::invalid_argument("member_id must be a UUID"))?,
            ),
            Some(Identifier::Email(email)) if !email.trim().is_empty() => {
                MemberIdentifier::Email(email)
            }
            Some(Identifier::DiscordId(id)) if !id.trim().is_empty() => {
                MemberIdentifier::DiscordId(id)
            }
            _ => {
                return Err(Status::invalid_argument(
                    "a non-empty member identifier is required",
                ));
            }
        };
        let member = self
            .repository
            .get(identifier)
            .await
            .map_err(database_status)?
            .ok_or_else(|| Status::not_found("member not found"))?;

        Ok(GetMemberResponse {
            member: Some(member.into()),
        }
        .into())
    }

    async fn list_members(
        &self,
        request: Request<ListMembersRequest>,
    ) -> Result<Response<ListMembersResponse>, Status> {
        let request = request.into_inner();
        let page_size = match request.page_size {
            size if size < 0 => {
                return Err(Status::invalid_argument("page_size must not be negative"));
            }
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        } as usize;
        let after = decode_page_token(&request.page_token, request.include_archived)?;
        let mut members = self
            .repository
            .list(request.include_archived, after, page_size as i64 + 1)
            .await
            .map_err(database_status)?;

        let next_page_token = if members.len() > page_size {
            members.truncate(page_size);
            encode_page_token(members[page_size - 1].id, request.include_archived)
        } else {
            String::new()
        };

        Ok(ListMembersResponse {
            members: members.into_iter().map(Into::into).collect(),
            next_page_token,
        }
        .into())
    }
}

fn encode_page_token(id: Uuid, include_archived: bool) -> String {
    let mut bytes = [0; 18];
    bytes[0] = 1;
    bytes[1] = u8::from(include_archived);
    bytes[2..].copy_from_slice(id.as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_page_token(token: &str, include_archived: bool) -> Result<Option<Uuid>, Status> {
    if token.is_empty() {
        return Ok(None);
    }
    let invalid = || Status::invalid_argument("invalid page_token for this request");
    if token.len() != 24 {
        return Err(invalid());
    }
    let bytes = URL_SAFE_NO_PAD.decode(token).map_err(|_| invalid())?;
    if bytes.len() != 18 || bytes[0] != 1 || bytes[1] != u8::from(include_archived) {
        return Err(invalid());
    }
    Uuid::from_slice(&bytes[2..])
        .map(Some)
        .map_err(|_| invalid())
}

fn database_status(error: sqlx::Error) -> Status {
    tracing::error!(%error, "member query failed");
    match error {
        sqlx::Error::PoolClosed
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_) => Status::unavailable("member storage unavailable"),
        _ => Status::internal("failed to query members"),
    }
}

impl From<StoredMember> for Member {
    fn from(member: StoredMember) -> Self {
        Self {
            member_id: member.id.to_string(),
            email: member.email,
            first_name: member.first_name,
            last_name: member.last_name,
            joined_at: Some(timestamp(member.joined_at)),
            discord_id: member.discord_id,
            discord_username: member.discord_username,
            updated_at: Some(timestamp(member.updated_at)),
            archived_at: member.archived_at.map(timestamp),
        }
    }
}

fn timestamp(value: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    }
}

#[cfg(test)]
mod tests;
