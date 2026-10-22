use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug, Clone, PartialEq)]
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
    /// `None` = dont change, `Some(None)` unlinks it
    pub discord_id: Option<Option<String>>,
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
        let now = Utc::now();

        sqlx::query_as!(
            Member,
            r#"
            INSERT INTO "Member"
                (email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername", "updatedAt")
            VALUES
                ($1, $2, $3, $4, $5, $6, $7)
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
            input.joined_at.unwrap_or(now),
            input.discord_id,
            input.discord_username,
            now
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

        let mut after = before.clone();
        if let Some(email) = input.email {
            after.email = email;
        }
        if let Some(first_name) = input.first_name {
            after.first_name = first_name;
        }
        if let Some(last_name) = input.last_name {
            after.last_name = last_name;
        }
        if let Some(joined_at) = input.joined_at {
            after.joined_at = joined_at;
        }
        if let Some(discord_id) = input.discord_id {
            after.discord_id = discord_id;
        }
        if let Some(discord_username) = input.discord_username {
            after.discord_username = Some(discord_username);
        }

        if skip_unchanged && after == before {
            return Ok(UpdateResult {
                before,
                after,
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
                "updatedAt" = $8
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
            after.email,
            after.first_name,
            after.last_name,
            after.joined_at,
            after.discord_id,
            after.discord_username,
            Utc::now()
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
        let updated = sqlx::query_as!(
            Member,
            r#"
            UPDATE
                "Member"
            SET
                "archivedAt" = $2,
                "updatedAt" = $2
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
            member_id,
            Utc::now()
        )
        .fetch_optional(&self.pool)
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
            .fetch_one(&self.pool)
            .await?;

            return Ok((existing, false));
        };

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
