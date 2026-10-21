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

pub struct CreateMember {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: Option<DateTime<Utc>>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
}

pub struct UpdateMember {
    pub member_id: i32,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub joined_at: Option<DateTime<Utc>>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
}

#[derive(Clone)]
pub enum OperationOutcome {
    Created(Member),
    Updated { before: Member, after: Member },
    Archived(Member),
    Unchanged(Member),
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

    pub async fn create_member(&self, input: CreateMember) -> Result<Member, sqlx::Error> {
        sqlx::query_as!(
            Member,
            r#"
            INSERT INTO "Member"
                (email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername", "updatedAt")
            VALUES
                ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP)
            RETURNING
                id,
                email,
                "firstName" AS first_name,
                "lastName" AS last_name,
                "joinedAt" AS joined_at,
                "discordId" AS discord_id,
                "discordUsername" AS discord_username,
                "updatedAt" AS updated_at,
                "archivedAt" AS archived_at
            "#,
            input.email,
            input.first_name,
            input.last_name,
            input.joined_at.unwrap_or_else(Utc::now),
            input.discord_id,
            input.discord_username
        )
        .fetch_one(&self.pool)
        .await
    }

    pub async fn update_member(&self, input: UpdateMember) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = self.update_in_transaction(&mut tx, input, false).await?;
        tx.commit().await?;
        Ok(result.after)
    }

    pub async fn update_operation(
        &self,
        input: UpdateMember,
    ) -> Result<OperationOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = self.update_in_transaction(&mut tx, input, true).await?;
        tx.commit().await?;
        Ok(result.into_outcome())
    }

    async fn update_in_transaction(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        input: UpdateMember,
        skip_unchanged: bool,
    ) -> Result<UpdateResult, sqlx::Error> {
        let before = sqlx::query_as!(
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
            FOR UPDATE
            "#,
            input.member_id
        )
        .fetch_one(&mut **tx)
        .await?;
        if skip_unchanged
            && input.email.as_ref().is_none_or(|v| *v == before.email)
            && input
                .first_name
                .as_ref()
                .is_none_or(|v| *v == before.first_name)
            && input
                .last_name
                .as_ref()
                .is_none_or(|v| *v == before.last_name)
            && input.joined_at.is_none_or(|v| v == before.joined_at)
            && input
                .discord_id
                .as_ref()
                .is_none_or(|v| Some(v) == before.discord_id.as_ref())
            && input
                .discord_username
                .as_ref()
                .is_none_or(|v| Some(v) == before.discord_username.as_ref())
        {
            return Ok(UpdateResult {
                before: before.clone(),
                after: before,
                changed: false,
            });
        }
        let after = sqlx::query_as!(
            Member,
            r#"
            UPDATE
                "Member"
            SET
                email = $2,
                "firstName" = $3,
                "lastName" = $4,
                "joinedAt" = $5,
                "discordId" = $6,
                "discordUsername" = $7,
                "updatedAt" = clock_timestamp()
            WHERE
                id = $1
            RETURNING
                id,
                email,
                "firstName" AS first_name,
                "lastName" AS last_name,
                "joinedAt" AS joined_at,
                "discordId" AS discord_id,
                "discordUsername" AS discord_username,
                "updatedAt" AS updated_at,
                "archivedAt" AS archived_at
            "#,
            input.member_id,
            input.email.unwrap_or_else(|| before.email.clone()),
            input
                .first_name
                .unwrap_or_else(|| before.first_name.clone()),
            input.last_name.unwrap_or_else(|| before.last_name.clone()),
            input.joined_at.unwrap_or(before.joined_at),
            input.discord_id.or_else(|| before.discord_id.clone()),
            input
                .discord_username
                .or_else(|| before.discord_username.clone())
        )
        .fetch_one(&mut **tx)
        .await?;
        Ok(UpdateResult {
            before,
            after,
            changed: true,
        })
    }

    pub async fn archive_member(&self, member_id: i32) -> Result<Member, sqlx::Error> {
        Ok(self.archive(member_id).await?.0)
    }

    pub async fn archive_operation(&self, member_id: i32) -> Result<OperationOutcome, sqlx::Error> {
        let (member, changed) = self.archive(member_id).await?;
        Ok(if changed {
            OperationOutcome::Archived(member)
        } else {
            OperationOutcome::Unchanged(member)
        })
    }

    async fn archive(&self, member_id: i32) -> Result<(Member, bool), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query_as!(
            Member,
            r#"
            UPDATE
                "Member"
            SET
                "archivedAt" = statement_timestamp(),
                "updatedAt" = statement_timestamp()
            WHERE
                id = $1
                AND "archivedAt" IS NULL
            RETURNING
                id,
                email,
                "firstName" AS first_name,
                "lastName" AS last_name,
                "joinedAt" AS joined_at,
                "discordId" AS discord_id,
                "discordUsername" AS discord_username,
                "updatedAt" AS updated_at,
                "archivedAt" AS archived_at
            "#,
            member_id
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(stored) = updated else {
            let existing = sqlx::query_as!(
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
                member_id
            )
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok((existing, false));
        };
        tx.commit().await?;
        Ok((stored, true))
    }
}

struct UpdateResult {
    before: Member,
    after: Member,
    changed: bool,
}

impl UpdateResult {
    fn into_outcome(self) -> OperationOutcome {
        if self.changed {
            OperationOutcome::Updated {
                before: self.before,
                after: self.after,
            }
        } else {
            OperationOutcome::Unchanged(self.after)
        }
    }
}
