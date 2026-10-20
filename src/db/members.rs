use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone)]
pub struct Member {
    pub id: i32,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: DateTime<Utc>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

pub enum MemberIdentifier {
    Id(i32),
    Email(String),
    DiscordId(String),
}

#[derive(Clone)]
pub struct MembersRepository {
    pool: PgPool,
}

impl MembersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, identifier: MemberIdentifier) -> Result<Option<Member>, sqlx::Error> {
        match identifier {
            MemberIdentifier::Id(id) => {
                sqlx::query_as!(
                    Member,
                    r#"
                    SELECT
                        id,
                        email,
                        "firstName" AS first_name,
                        "lastName" AS last_name,
                        "joinedAt" AS joined_at,
                        "discordId" AS discord_id,
                        "discordUsername" AS discord_username,
                        "updatedAt" AS updated_at,
                        "archivedAt" AS archived_at
                    FROM
                        "Member"
                    WHERE
                        id = $1
                    "#,
                    id
                )
                .fetch_optional(&self.pool)
                .await
            }
            MemberIdentifier::Email(email) => {
                sqlx::query_as!(
                    Member,
                    r#"
                    SELECT
                        id,
                        email,
                        "firstName" AS first_name,
                        "lastName" AS last_name,
                        "joinedAt" AS joined_at,
                        "discordId" AS discord_id,
                        "discordUsername" AS discord_username,
                        "updatedAt" AS updated_at,
                        "archivedAt" AS archived_at
                    FROM
                        "Member"
                    WHERE
                        email = $1
                    "#,
                    email
                )
                .fetch_optional(&self.pool)
                .await
            }
            MemberIdentifier::DiscordId(id) => {
                sqlx::query_as!(
                    Member,
                    r#"
                    SELECT
                        id,
                        email,
                        "firstName" AS first_name,
                        "lastName" AS last_name,
                        "joinedAt" AS joined_at,
                        "discordId" AS discord_id,
                        "discordUsername" AS discord_username,
                        "updatedAt" AS updated_at,
                        "archivedAt" AS archived_at
                    FROM
                        "Member"
                    WHERE
                        "discordId" = $1
                    "#,
                    id
                )
                .fetch_optional(&self.pool)
                .await
            }
        }
    }

    pub async fn list(
        &self,
        include_archived: bool,
        after: Option<i32>,
        limit: i64,
    ) -> Result<Vec<Member>, sqlx::Error> {
        sqlx::query_as!(
            Member,
            r#"
            SELECT
                id,
                email,
                "firstName" AS first_name,
                "lastName" AS last_name,
                "joinedAt" AS joined_at,
                "discordId" AS discord_id,
                "discordUsername" AS discord_username,
                "updatedAt" AS updated_at,
                "archivedAt" AS archived_at
            FROM
                "Member"
            WHERE
                ($1 OR "archivedAt" IS NULL)
                AND ($2::integer IS NULL OR id > $2)
            ORDER BY
                id ASC
            LIMIT
                $3
            "#,
            include_archived,
            after,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }
}
