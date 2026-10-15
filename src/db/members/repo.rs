use uuid::Uuid;

use crate::db::members::model::{Member, NewMember};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberQuery {
    pub include_archived: bool,
    pub limit: i64,
    pub after_id: Option<Uuid>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("email is already in use")]
    DuplicateEmail,
    #[error("discord id is already in use")]
    DuplicateDiscordId,
    #[error("{0}")]
    Database(String),
}

#[allow(async_fn_in_trait)]
pub trait MemberRepo: Send + Sync {
    async fn find_by_id(&self, member_id: Uuid) -> Result<Option<Member>, RepoError>;

    async fn find_by_email(&self, email: &str) -> Result<Option<Member>, RepoError>;

    async fn find_by_discord_id(&self, discord_id: &str) -> Result<Option<Member>, RepoError>;

    async fn list(&self, query: MemberQuery) -> Result<Vec<Member>, RepoError>;

    async fn insert(&self, member: NewMember) -> Result<Member, RepoError>;

    async fn update(&self, member: Member) -> Result<Option<Member>, RepoError>;
}
