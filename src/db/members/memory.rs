use std::sync::Mutex;

use uuid::Uuid;

use crate::db::members::model::{Member, NewMember};
use crate::db::members::repo::{MemberQuery, MemberRepo, RepoError};

#[derive(Debug, Default)]
pub struct MemoryMemberRepo {
    members: Mutex<Vec<Member>>,
}

impl MemberRepo for MemoryMemberRepo {
    async fn find_by_id(&self, member_id: Uuid) -> Result<Option<Member>, RepoError> {
        Ok(self
            .members
            .lock()
            .expect("member repo lock")
            .iter()
            .find(|member| member.member_id == member_id)
            .cloned())
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<Member>, RepoError> {
        Ok(self
            .members
            .lock()
            .expect("member repo lock")
            .iter()
            .find(|member| member.email == email)
            .cloned())
    }

    async fn find_by_discord_id(&self, discord_id: &str) -> Result<Option<Member>, RepoError> {
        Ok(self
            .members
            .lock()
            .expect("member repo lock")
            .iter()
            .find(|member| member.discord_id.as_deref() == Some(discord_id))
            .cloned())
    }

    async fn list(&self, query: MemberQuery) -> Result<Vec<Member>, RepoError> {
        let members = self.members.lock().expect("member repo lock");
        let mut rows: Vec<Member> = members
            .iter()
            .filter(|member| query.include_archived || member.archived_at.is_none())
            .filter(|member| {
                query
                    .after_id
                    .is_none_or(|after_id| member.member_id > after_id)
            })
            .cloned()
            .collect();
        drop(members);

        rows.sort_by_key(|member| member.member_id);
        let limit = usize::try_from(query.limit).unwrap_or(0);
        rows.truncate(limit);
        Ok(rows)
    }

    async fn insert(&self, member: NewMember) -> Result<Member, RepoError> {
        let mut members = self.members.lock().expect("member repo lock");
        if members
            .iter()
            .any(|existing| existing.email == member.email)
        {
            return Err(RepoError::DuplicateEmail);
        }
        if let Some(discord_id) = &member.discord_id
            && members
                .iter()
                .any(|existing| existing.discord_id.as_ref() == Some(discord_id))
        {
            return Err(RepoError::DuplicateDiscordId);
        }

        let stored = Member {
            member_id: member.member_id,
            email: member.email,
            first_name: member.first_name,
            last_name: member.last_name,
            joined_at: member.joined_at,
            discord_id: member.discord_id,
            discord_username: member.discord_username,
            updated_at: member.updated_at,
            archived_at: None,
        };
        members.push(stored.clone());
        Ok(stored)
    }

    async fn update(&self, member: Member) -> Result<Option<Member>, RepoError> {
        let mut members = self.members.lock().expect("member repo lock");
        if members.iter().any(|existing| {
            existing.member_id != member.member_id && existing.email == member.email
        }) {
            return Err(RepoError::DuplicateEmail);
        }
        if let Some(discord_id) = &member.discord_id
            && members.iter().any(|existing| {
                existing.member_id != member.member_id
                    && existing.discord_id.as_ref() == Some(discord_id)
            })
        {
            return Err(RepoError::DuplicateDiscordId);
        }

        let Some(slot) = members
            .iter_mut()
            .find(|existing| existing.member_id == member.member_id)
        else {
            return Ok(None);
        };
        *slot = member.clone();
        Ok(Some(member))
    }
}
