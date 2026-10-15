use jiff::Timestamp;
use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::db::members::model::{Member, NewMember};
use crate::db::members::repo::{MemberQuery, MemberRepo, RepoError};

#[derive(Debug, Clone)]
pub struct MemberSqlRepo {
    pool: PgPool,
}

struct MemberRow {
    member_id: Uuid,
    email: String,
    first_name: String,
    last_name: String,
    joined_at: DateTime<Utc>,
    discord_id: Option<String>,
    discord_username: Option<String>,
    updated_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

impl MemberSqlRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

impl MemberRepo for MemberSqlRepo {
    async fn find_by_id(&self, member_id: Uuid) -> Result<Option<Member>, RepoError> {
        let row = sqlx::query_as!(
            MemberRow,
            r#"
            SELECT
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at,
                archived_at
            FROM members
            WHERE member_id = $1
            "#,
            member_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(row.map(Member::from))
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<Member>, RepoError> {
        let row = sqlx::query_as!(
            MemberRow,
            r#"
            SELECT
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at,
                archived_at
            FROM members
            WHERE email = $1
            "#,
            email,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(row.map(Member::from))
    }

    async fn find_by_discord_id(&self, discord_id: &str) -> Result<Option<Member>, RepoError> {
        let row = sqlx::query_as!(
            MemberRow,
            r#"
            SELECT
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at,
                archived_at
            FROM members
            WHERE discord_id = $1
            "#,
            discord_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(row.map(Member::from))
    }

    async fn list(&self, query: MemberQuery) -> Result<Vec<Member>, RepoError> {
        let after_id = query.after_id.unwrap_or(Uuid::nil());
        let rows = sqlx::query_as!(
            MemberRow,
            r#"
            SELECT
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at,
                archived_at
            FROM members
            WHERE ($1::bool OR archived_at IS NULL)
              AND member_id > $2
            ORDER BY member_id
            LIMIT $3
            "#,
            query.include_archived,
            after_id,
            query.limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(rows.into_iter().map(Member::from).collect())
    }

    async fn insert(&self, member: NewMember) -> Result<Member, RepoError> {
        let row = sqlx::query_as!(
            MemberRow,
            r#"
            INSERT INTO members (
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at,
                archived_at
            "#,
            member.member_id,
            member.email,
            member.first_name,
            member.last_name,
            timestamp_to_db(member.joined_at),
            member.discord_id,
            member.discord_username,
            timestamp_to_db(member.updated_at),
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(Member::from(row))
    }

    async fn update(&self, member: Member) -> Result<Option<Member>, RepoError> {
        let row = sqlx::query_as!(
            MemberRow,
            r#"
            UPDATE members
            SET
                email = $2,
                first_name = $3,
                last_name = $4,
                joined_at = $5,
                discord_id = $6,
                discord_username = $7,
                updated_at = $8,
                archived_at = $9
            WHERE member_id = $1
            RETURNING
                member_id,
                email,
                first_name,
                last_name,
                joined_at,
                discord_id,
                discord_username,
                updated_at,
                archived_at
            "#,
            member.member_id,
            member.email,
            member.first_name,
            member.last_name,
            timestamp_to_db(member.joined_at),
            member.discord_id,
            member.discord_username,
            timestamp_to_db(member.updated_at),
            member.archived_at.map(timestamp_to_db),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db)?;

        Ok(row.map(Member::from))
    }
}

impl From<MemberRow> for Member {
    fn from(row: MemberRow) -> Self {
        Self {
            member_id: row.member_id,
            email: row.email,
            first_name: row.first_name,
            last_name: row.last_name,
            joined_at: timestamp_from_db(row.joined_at),
            discord_id: row.discord_id,
            discord_username: row.discord_username,
            updated_at: timestamp_from_db(row.updated_at),
            archived_at: row.archived_at.map(timestamp_from_db),
        }
    }
}

fn map_db(err: sqlx::Error) -> RepoError {
    let Some(db_err) = err.as_database_error() else {
        return RepoError::Database(err.to_string());
    };
    if db_err.code().as_deref() != Some("23505") {
        return RepoError::Database(err.to_string());
    }

    match db_err.constraint() {
        Some("members_email_key") => RepoError::DuplicateEmail,
        Some("members_discord_id_key") => RepoError::DuplicateDiscordId,
        _ => RepoError::Database(err.to_string()),
    }
}

fn timestamp_to_db(value: Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp(value.as_second(), value.subsec_nanosecond() as u32)
        .expect("member timestamp is in range")
}

fn timestamp_from_db(value: DateTime<Utc>) -> Timestamp {
    Timestamp::new(value.timestamp(), value.timestamp_subsec_nanos() as i32)
        .expect("database timestamp is in range")
}
