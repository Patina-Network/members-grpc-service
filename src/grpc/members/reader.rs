use std::marker::PhantomData;

use tonic::Status;

use crate::{
    db::members::MembersRepository,
    domain::members::{Member as BaseMember, MemberIdentifier, MemberRole as StoredRole},
    grpc::{
        database_status, page::Page, timestamp, validate_discord_id, validate_email,
        validate_member_id,
    },
    proto::members::{
        GetMemberRequest, ListMembersRequest, Member, MemberRole as ProtoRole,
        admin::SensitiveMember, get_member_request::Identifier,
    },
};

pub struct MembersReader<R, T> {
    repository: R,
    // T lets us lock down which `Member` type to return
    // but Rust compiles generics, so we add this bullshit field
    // so it will compile
    _placeholder: PhantomData<fn() -> T>,
}

impl<R: MembersRepository, T: From<BaseMember>> MembersReader<R, T> {
    pub fn new(repository: R) -> Self {
        Self {
            repository,
            _placeholder: PhantomData,
        }
    }

    pub async fn get(&self, request: GetMemberRequest) -> Result<T, Status> {
        let identifier = match request.identifier {
            Some(Identifier::MemberId(id)) => {
                validate_member_id(id)?;
                MemberIdentifier::Id(id)
            }
            Some(Identifier::Email(email)) => {
                validate_email(&email)?;
                MemberIdentifier::Email(email)
            }
            Some(Identifier::DiscordId(id)) => {
                validate_discord_id(&id)?;
                MemberIdentifier::DiscordId(id)
            }
            None => {
                return Err(Status::invalid_argument("a member identifier is required"));
            }
        };

        self.repository
            .get(identifier)
            .await
            .map_err(database_status)?
            .map(Into::into)
            .ok_or(Status::not_found("member not found"))
    }

    pub async fn list(&self, request: ListMembersRequest) -> Result<(Vec<T>, String), Status> {
        let page = Page::new(
            request.page_size,
            &request.page_token,
            if request.include_archived { "1" } else { "0" },
        )?;

        let members = self
            .repository
            .list(request.include_archived, page.after(), page.fetch_limit())
            .await
            .map_err(database_status)?;

        let (members, next_page_token) = page.finish(members, |member| member.id);

        Ok((
            members.into_iter().map(Into::into).collect(),
            next_page_token,
        ))
    }
}

impl From<StoredRole> for ProtoRole {
    fn from(role: StoredRole) -> Self {
        match role {
            StoredRole::Board => Self::Board,
            StoredRole::Mentee => Self::Mentee,
            StoredRole::Mentor => Self::Mentor,
        }
    }
}

impl From<BaseMember> for Member {
    fn from(member: BaseMember) -> Self {
        Self {
            member_id: member.id,
            email: member.email,
            first_name: member.first_name,
            last_name: member.last_name,
            joined_at: Some(timestamp(member.joined_at)),
            discord_id: member.discord_id,
            discord_username: member.discord_username,
            updated_at: Some(timestamp(member.updated_at)),
            archived_at: member.archived_at.map(timestamp),
            linkedin_username: member.linkedin_username,
            instagram_username: member.instagram_username,
            career: member.career,
            industry: member.industry,
            role: member.role.map(|role| ProtoRole::from(role).into()),
        }
    }
}

impl From<BaseMember> for SensitiveMember {
    fn from(member: BaseMember) -> Self {
        Self {
            member_id: member.id,
            email: member.email,
            first_name: member.first_name,
            last_name: member.last_name,
            joined_at: Some(timestamp(member.joined_at)),
            discord_id: member.discord_id,
            discord_username: member.discord_username,
            updated_at: Some(timestamp(member.updated_at)),
            archived_at: member.archived_at.map(timestamp),
            linkedin_username: member.linkedin_username,
            instagram_username: member.instagram_username,
            career: member.career,
            industry: member.industry,
            profile: member.profile,
            role: member.role.map(|role| ProtoRole::from(role).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;
    use prost_types::{DescriptorProto, FieldDescriptorProto, FileDescriptorSet};

    use crate::proto::FILE_DESCRIPTOR_SET;

    fn message<'a>(set: &'a FileDescriptorSet, package: &str, name: &str) -> &'a DescriptorProto {
        set.file
            .iter()
            .filter(|file| file.package() == package)
            .flat_map(|file| &file.message_type)
            .find(|message| message.name() == name)
            .unwrap_or_else(|| panic!("{package}.{name} is not in the descriptor set"))
    }

    fn shape(field: &FieldDescriptorProto) -> FieldDescriptorProto {
        FieldDescriptorProto {
            oneof_index: None,
            ..field.clone()
        }
    }

    #[test]
    fn sensitive_member_has_every_member_field() {
        let set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).unwrap();
        let member = message(&set, "members", "Member");
        let sensitive: Vec<_> = message(&set, "members.admin", "SensitiveMember")
            .field
            .iter()
            .map(shape)
            .collect();

        let missing: Vec<_> = member
            .field
            .iter()
            .filter(|field| !sensitive.contains(&shape(field)))
            .map(|field| format!("{} = {}", field.name(), field.number()))
            .collect();

        assert!(
            missing.is_empty(),
            "SensitiveMember must declare these Member fields with the same name, number, and type: {missing:?}"
        );
    }
}
