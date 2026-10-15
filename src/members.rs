use jiff::Timestamp;
use uuid::Uuid;

use crate::db::members::{Member, MemberQuery, MemberRepo, NewMember, RepoError};

const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone)]
pub struct MemberService<R> {
    repo: R,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateMember {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: Option<Timestamp>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Set<T> {
    Keep,
    Value(T),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberUpdate {
    pub member_id: Uuid,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub joined_at: Option<Timestamp>,
    pub discord_id: Set<String>,
    pub discord_username: Set<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberLookup {
    Id(Uuid),
    Email(String),
    DiscordId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListMembers {
    pub include_archived: bool,
    pub page_size: i64,
    pub after_id: Option<Uuid>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MemberError {
    #[error("email is already in use")]
    DuplicateEmail,
    #[error("discord id is already in use")]
    DuplicateDiscordId,
    #[error("member not found")]
    NotFound,
    #[error("email, first name, and last name are required")]
    Invalid,
    #[error("{0}")]
    Repo(String),
}

impl From<RepoError> for MemberError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::DuplicateEmail => Self::DuplicateEmail,
            RepoError::DuplicateDiscordId => Self::DuplicateDiscordId,
            RepoError::Database(message) => Self::Repo(message),
        }
    }
}

impl<R: MemberRepo> MemberService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn create(&self, input: CreateMember) -> Result<Member, MemberError> {
        let email = required(&input.email)?;
        let first_name = required(&input.first_name)?;
        let last_name = required(&input.last_name)?;
        let discord_id = blank_as_none(input.discord_id.as_deref());
        let discord_username = blank_as_none(input.discord_username.as_deref());

        if self.repo.find_by_email(&email).await?.is_some() {
            return Err(MemberError::DuplicateEmail);
        }
        if let Some(discord_id) = &discord_id
            && self.repo.find_by_discord_id(discord_id).await?.is_some()
        {
            return Err(MemberError::DuplicateDiscordId);
        }

        let now = Timestamp::now();
        self.repo
            .insert(NewMember {
                member_id: Uuid::new_v4(),
                email,
                first_name,
                last_name,
                joined_at: input.joined_at.unwrap_or(now),
                discord_id,
                discord_username,
                updated_at: now,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn get(&self, lookup: MemberLookup) -> Result<Member, MemberError> {
        let member = match lookup {
            MemberLookup::Id(member_id) => self.repo.find_by_id(member_id).await?,
            MemberLookup::Email(email) => {
                let email = required(&email)?;
                self.repo.find_by_email(&email).await?
            }
            MemberLookup::DiscordId(discord_id) => {
                let Some(discord_id) = blank_as_none(Some(&discord_id)) else {
                    return Err(MemberError::Invalid);
                };
                self.repo.find_by_discord_id(&discord_id).await?
            }
        };
        member.ok_or(MemberError::NotFound)
    }

    pub async fn list(&self, query: ListMembers) -> Result<Vec<Member>, MemberError> {
        self.repo
            .list(MemberQuery {
                include_archived: query.include_archived,
                limit: page_limit(query.page_size),
                after_id: query.after_id,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn update(&self, update: MemberUpdate) -> Result<Member, MemberError> {
        let Some(mut member) = self.repo.find_by_id(update.member_id).await? else {
            return Err(MemberError::NotFound);
        };

        if let Some(email) = update.email {
            let email = required(&email)?;
            if email != member.email {
                if self.repo.find_by_email(&email).await?.is_some() {
                    return Err(MemberError::DuplicateEmail);
                }
                member.email = email;
            }
        }
        if let Some(first_name) = update.first_name {
            member.first_name = required(&first_name)?;
        }
        if let Some(last_name) = update.last_name {
            member.last_name = required(&last_name)?;
        }
        if let Some(joined_at) = update.joined_at {
            member.joined_at = joined_at;
        }
        member.discord_id = apply_text(member.discord_id, update.discord_id);
        if let Some(discord_id) = &member.discord_id
            && let Some(existing) = self.repo.find_by_discord_id(discord_id).await?
            && existing.member_id != member.member_id
        {
            return Err(MemberError::DuplicateDiscordId);
        }
        member.discord_username = apply_text(member.discord_username, update.discord_username);
        member.updated_at = Timestamp::now();

        self.repo.update(member).await?.ok_or(MemberError::NotFound)
    }

    pub async fn archive(&self, member_id: Uuid) -> Result<Member, MemberError> {
        let Some(mut member) = self.repo.find_by_id(member_id).await? else {
            return Err(MemberError::NotFound);
        };
        if member.archived_at.is_some() {
            return Ok(member);
        }

        let now = Timestamp::now();
        member.archived_at = Some(now);
        member.updated_at = now;
        self.repo.update(member).await?.ok_or(MemberError::NotFound)
    }
}

fn page_limit(requested: i64) -> i64 {
    if requested <= 0 {
        DEFAULT_PAGE_SIZE
    } else {
        requested.min(MAX_PAGE_SIZE)
    }
}

fn required(value: &str) -> Result<String, MemberError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(MemberError::Invalid)
    } else {
        Ok(trimmed.to_owned())
    }
}

fn blank_as_none(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn apply_text(current: Option<String>, update: Set<String>) -> Option<String> {
    match update {
        Set::Keep => current,
        Set::Clear => None,
        Set::Value(value) => blank_as_none(Some(&value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::members::MemoryMemberRepo;

    fn service() -> MemberService<MemoryMemberRepo> {
        MemberService::new(MemoryMemberRepo::default())
    }

    fn sample() -> CreateMember {
        CreateMember {
            email: " ada@example.com ".into(),
            first_name: " Ada ".into(),
            last_name: " Lovelace ".into(),
            joined_at: None,
            discord_id: Some("  ".into()),
            discord_username: None,
        }
    }

    fn keep_update(member_id: Uuid) -> MemberUpdate {
        MemberUpdate {
            member_id,
            email: None,
            first_name: None,
            last_name: None,
            joined_at: None,
            discord_id: Set::Keep,
            discord_username: Set::Keep,
        }
    }

    #[tokio::test]
    async fn create_trims_fields_and_leaves_discord_empty() {
        let service = service();
        let created = tokio::spawn(async move { service.create(sample()).await })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(created.email, "ada@example.com");
        assert_eq!(created.first_name, "Ada");
        assert_eq!(created.last_name, "Lovelace");
        assert_eq!(created.discord_id, None);
        assert_eq!(created.archived_at, None);
        assert_eq!(created.joined_at, created.updated_at);
    }

    #[tokio::test]
    async fn create_rejects_a_duplicate_email_or_discord_id() {
        let service = service();
        service.create(sample()).await.unwrap();

        let duplicate_email = service.create(sample()).await.unwrap_err();
        assert_eq!(duplicate_email, MemberError::DuplicateEmail);

        let mut with_discord = sample();
        with_discord.email = "grace@example.com".into();
        with_discord.discord_id = Some("123".into());
        service.create(with_discord.clone()).await.unwrap();

        with_discord.email = "other@example.com".into();
        let duplicate_discord = service.create(with_discord).await.unwrap_err();
        assert_eq!(duplicate_discord, MemberError::DuplicateDiscordId);
    }

    #[tokio::test]
    async fn create_rejects_a_blank_name() {
        let service = service();
        let mut input = sample();
        input.first_name = " ".into();
        assert_eq!(
            service.create(input).await.unwrap_err(),
            MemberError::Invalid
        );
    }

    #[tokio::test]
    async fn get_finds_by_email_and_misses_unknown_members() {
        let service = service();
        let created = service.create(sample()).await.unwrap();

        let found = service
            .get(MemberLookup::Email(" ada@example.com ".into()))
            .await
            .unwrap();
        assert_eq!(found.member_id, created.member_id);

        let mut with_discord = sample();
        with_discord.email = "grace@example.com".into();
        with_discord.discord_id = Some(" 123 ".into());
        let with_discord = service.create(with_discord).await.unwrap();
        let by_discord = service
            .get(MemberLookup::DiscordId("123".into()))
            .await
            .unwrap();
        assert_eq!(by_discord.member_id, with_discord.member_id);

        let missing = service
            .get(MemberLookup::Id(Uuid::nil()))
            .await
            .unwrap_err();
        assert_eq!(missing, MemberError::NotFound);
    }

    #[tokio::test]
    async fn update_changes_only_the_fields_that_were_set() {
        let service = service();
        let created = service.create(sample()).await.unwrap();
        let joined_at = Timestamp::from_second(1_700_000_000).unwrap();

        let mut update = keep_update(created.member_id);
        update.first_name = Some("Augusta".into());
        update.joined_at = Some(joined_at);
        update.discord_id = Set::Value("999".into());
        let updated = service.update(update).await.unwrap();

        assert_eq!(updated.first_name, "Augusta");
        assert_eq!(updated.last_name, "Lovelace");
        assert_eq!(updated.email, "ada@example.com");
        assert_eq!(updated.joined_at, joined_at);
        assert_eq!(updated.discord_id.as_deref(), Some("999"));
        assert!(updated.updated_at > created.updated_at);
    }

    #[tokio::test]
    async fn update_can_clear_discord_and_rejects_a_taken_email() {
        let service = service();
        let mut first = sample();
        first.discord_id = Some("111".into());
        let first = service.create(first).await.unwrap();
        let mut second = sample();
        second.email = "grace@example.com".into();
        second.discord_id = Some("222".into());
        let second = service.create(second).await.unwrap();

        let mut clear = keep_update(first.member_id);
        clear.discord_id = Set::Clear;
        let cleared = service.update(clear).await.unwrap();
        assert_eq!(cleared.discord_id, None);

        let mut taken = keep_update(second.member_id);
        taken.email = Some("ada@example.com".into());
        assert_eq!(
            service.update(taken).await.unwrap_err(),
            MemberError::DuplicateEmail
        );

        assert_eq!(
            service.update(keep_update(Uuid::nil())).await.unwrap_err(),
            MemberError::NotFound
        );
    }

    #[tokio::test]
    async fn list_skips_archived_members_until_asked() {
        let service = service();
        let active = service.create(sample()).await.unwrap();
        let mut other = sample();
        other.email = "grace@example.com".into();
        let archived = service.create(other).await.unwrap();
        service.archive(archived.member_id).await.unwrap();

        let visible = service
            .list(ListMembers {
                include_archived: false,
                page_size: 10,
                after_id: None,
            })
            .await
            .unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].member_id, active.member_id);

        let all = service
            .list(ListMembers {
                include_archived: true,
                page_size: 10,
                after_id: None,
            })
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn archive_is_idempotent_and_missing_members_are_not_found() {
        let service = service();
        let created = service.create(sample()).await.unwrap();
        let archived = service.archive(created.member_id).await.unwrap();
        let again = service.archive(created.member_id).await.unwrap();

        assert_eq!(archived.archived_at, again.archived_at);
        assert!(archived.archived_at.is_some());
        assert_eq!(
            service.archive(Uuid::nil()).await.unwrap_err(),
            MemberError::NotFound
        );
    }

    #[tokio::test]
    async fn list_pages_by_member_id() {
        let service = service();
        service.create(sample()).await.unwrap();
        let mut other = sample();
        other.email = "grace@example.com".into();
        service.create(other).await.unwrap();

        let first = service
            .list(ListMembers {
                include_archived: false,
                page_size: 1,
                after_id: None,
            })
            .await
            .unwrap();
        let second = service
            .list(ListMembers {
                include_archived: false,
                page_size: 0,
                after_id: Some(first[0].member_id),
            })
            .await
            .unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first[0].member_id, second[0].member_id);
        assert!(second[0].member_id > first[0].member_id);
    }

    #[test]
    fn page_limit_uses_a_default_and_a_cap() {
        assert_eq!(page_limit(0), DEFAULT_PAGE_SIZE);
        assert_eq!(page_limit(-5), DEFAULT_PAGE_SIZE);
        assert_eq!(page_limit(10), 10);
        assert_eq!(page_limit(1_000), MAX_PAGE_SIZE);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL and the members database"]
    async fn sql_repo_create_and_get() {
        let url = std::env::var("DATABASE_URL").unwrap();
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        crate::db::client::migrate(&pool).await.unwrap();

        let service = MemberService::new(crate::db::members::MemberSqlRepo::new(pool.clone()));
        let email = format!("sql-test-{}@example.com", Uuid::new_v4());
        let created = service
            .create(CreateMember {
                email: email.clone(),
                first_name: "Ada".into(),
                last_name: "Lovelace".into(),
                joined_at: None,
                discord_id: Some(format!("discord-{}", Uuid::new_v4())),
                discord_username: Some("ada".into()),
            })
            .await
            .unwrap();

        let found = service.get(MemberLookup::Email(email)).await.unwrap();
        assert_eq!(found, created);

        sqlx::query(r#"DELETE FROM "Member" WHERE id = $1"#)
            .bind(created.member_id)
            .execute(&pool)
            .await
            .unwrap();
    }
}
