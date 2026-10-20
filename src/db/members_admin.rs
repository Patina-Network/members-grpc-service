use sqlx::PgPool;

use super::members::Member;

pub struct CreateMember {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: Option<i64>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
}

pub struct UpdateMember {
    pub member_id: i32,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub joined_at: Option<i64>,
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

pub type EventEncoder = fn(&OperationOutcome) -> (&'static str, Vec<u8>);

#[derive(Clone)]
pub struct MembersAdminRepository {
    pool: PgPool,
    encode_event: EventEncoder,
}

impl MembersAdminRepository {
    pub fn new(pool: PgPool, encode_event: EventEncoder) -> Self {
        Self { pool, encode_event }
    }

    pub async fn create_member(&self, input: CreateMember) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let stored = self
            .insert_member(&mut tx, &input, false)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        tx.commit().await?;
        Ok(stored)
    }

    async fn insert_member(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        input: &CreateMember,
        upsert: bool,
    ) -> Result<Option<Member>, sqlx::Error> {
        let stored = if upsert {
            sqlx::query_as!(
                Member,
                r#"
                INSERT INTO "Member"
                    (email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername", "updatedAt")
                VALUES
                    (
                        $1, $2, $3,
                        COALESCE(
                            to_timestamp(($4::bigint / 1000000)::double precision)
                                + ($4::bigint % 1000000) * interval '1 microsecond',
                            CURRENT_TIMESTAMP
                        ),
                        $5, $6, CURRENT_TIMESTAMP
                    )
                ON CONFLICT (email) DO NOTHING
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
                input.joined_at,
                input.discord_id,
                input.discord_username
            )
            .fetch_optional(&mut **tx)
            .await?
        } else {
            sqlx::query_as!(
                Member,
                r#"
                INSERT INTO "Member"
                    (email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername", "updatedAt")
                VALUES
                    (
                        $1, $2, $3,
                        COALESCE(
                            to_timestamp(($4::bigint / 1000000)::double precision)
                                + ($4::bigint % 1000000) * interval '1 microsecond',
                            CURRENT_TIMESTAMP
                        ),
                        $5, $6, CURRENT_TIMESTAMP
                    )
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
                input.joined_at,
                input.discord_id,
                input.discord_username
            )
            .fetch_optional(&mut **tx)
            .await?
        };
        if let Some(member) = &stored {
            self.publish(tx, &OperationOutcome::Created(member.clone()))
                .await?;
        }
        Ok(stored)
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
            && input
                .joined_at
                .is_none_or(|v| v == before.joined_at.timestamp_micros())
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
                email = COALESCE($2, email),
                "firstName" = COALESCE($3, "firstName"),
                "lastName" = COALESCE($4, "lastName"),
                "joinedAt" = COALESCE(
                    to_timestamp(($5::bigint / 1000000)::double precision)
                        + ($5::bigint % 1000000) * interval '1 microsecond',
                    "joinedAt"
                ),
                "discordId" = COALESCE($6, "discordId"),
                "discordUsername" = COALESCE($7, "discordUsername"),
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
            input.email,
            input.first_name,
            input.last_name,
            input.joined_at,
            input.discord_id,
            input.discord_username
        )
        .fetch_one(&mut **tx)
        .await?;
        let result = UpdateResult {
            before,
            after,
            changed: true,
        };
        self.publish(
            tx,
            &OperationOutcome::Updated {
                before: result.before.clone(),
                after: result.after.clone(),
            },
        )
        .await?;
        Ok(result)
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
        self.publish(&mut tx, &OperationOutcome::Archived(stored.clone()))
            .await?;
        tx.commit().await?;
        Ok((stored, true))
    }

    pub async fn upsert_member(
        &self,
        input: CreateMember,
    ) -> Result<OperationOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        if let Some(stored) = self.insert_member(&mut tx, &input, true).await? {
            tx.commit().await?;
            return Ok(OperationOutcome::Created(stored));
        }
        let id = sqlx::query_scalar!(
            r#"
            SELECT
                id
            FROM
                "Member"
            WHERE
                email = $1
            FOR UPDATE
            "#,
            &input.email
        )
        .fetch_one(&mut *tx)
        .await?;
        let result = self
            .update_in_transaction(
                &mut tx,
                UpdateMember {
                    member_id: id,
                    email: Some(input.email),
                    first_name: Some(input.first_name),
                    last_name: Some(input.last_name),
                    joined_at: input.joined_at,
                    discord_id: input.discord_id,
                    discord_username: input.discord_username,
                },
                true,
            )
            .await?;
        tx.commit().await?;
        Ok(result.into_outcome())
    }

    async fn publish(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        outcome: &OperationOutcome,
    ) -> Result<(), sqlx::Error> {
        let (name, payload) = (self.encode_event)(outcome);
        let id = sqlx::query_scalar!(
            r#"
            INSERT INTO "MemberEvents"
                ("eventType", payload)
            VALUES
                ($1, $2)
            RETURNING
                id
            "#,
            name,
            payload
        )
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query!("SELECT pg_notify($1, $2)", name, id.to_string())
            .execute(&mut **tx)
            .await?;
        Ok(())
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
