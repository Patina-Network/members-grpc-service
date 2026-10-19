use prost::Message;
use sqlx::{PgPool, Row};

use crate::proto::members::{
    Member,
    admin::{
        ArchiveMemberRequest, CreateMemberRequest, MemberArchived, MemberCreated, MemberUnchanged,
        MemberUpdated, UpdateMemberRequest, member_operation_result::Outcome,
    },
};

#[derive(Clone)]
pub struct MembersAdminRepository {
    pool: PgPool,
}

impl MembersAdminRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_member(
        &self,
        input: CreateMemberRequest,
        joined: Option<i64>,
    ) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let stored = Self::insert_member(&mut tx, input, joined, false)
            .await?
            .ok_or(sqlx::Error::RowNotFound)?;
        tx.commit().await?;
        Ok(stored)
    }

    async fn insert_member(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        input: CreateMemberRequest,
        joined: Option<i64>,
        upsert: bool,
    ) -> Result<Option<Member>, sqlx::Error> {
        let sql = r#"
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
                id AS member_id,
                email,
                "firstName",
                "lastName",
                "discordId",
                "discordUsername",
                "joinedAt" AS joined,
                "updatedAt" AS updated,
                "archivedAt" AS archived
            "#;
        let sql = if upsert {
            sql.replace(
                "RETURNING",
                "ON CONFLICT (email) DO NOTHING\n            RETURNING",
            )
        } else {
            sql.to_owned()
        };
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(input.email)
            .bind(input.first_name)
            .bind(input.last_name)
            .bind(joined)
            .bind(input.discord_id)
            .bind(input.discord_username)
            .fetch_optional(&mut **tx)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let stored = member(row)?;
        publish(
            tx,
            "MemberCreated",
            MemberCreated {
                member: Some(stored.clone()),
            }
            .encode_to_vec(),
        )
        .await?;
        Ok(Some(stored))
    }

    pub async fn update_member(
        &self,
        input: UpdateMemberRequest,
        joined: Option<i64>,
    ) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let outcome = Self::update_in_transaction(&mut tx, input, joined, false).await?;
        tx.commit().await?;
        let Outcome::Updated(event) = outcome else {
            unreachable!()
        };
        Ok(event.after.expect("updated member"))
    }

    pub async fn update_operation(
        &self,
        input: UpdateMemberRequest,
        joined: Option<i64>,
    ) -> Result<Outcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let outcome = Self::update_in_transaction(&mut tx, input, joined, true).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    async fn update_in_transaction(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        input: UpdateMemberRequest,
        joined: Option<i64>,
        skip_unchanged: bool,
    ) -> Result<Outcome, sqlx::Error> {
        let sql = r#"
            SELECT
                id AS member_id,
                email,
                "firstName",
                "lastName",
                "discordId",
                "discordUsername",
                "joinedAt" AS joined,
                "updatedAt" AS updated,
                "archivedAt" AS archived
            FROM
                "Member"
            WHERE
                id = $1
            FOR UPDATE
            "#;
        let before = member(
            sqlx::query(sql)
                .bind(input.member_id)
                .fetch_optional(&mut **tx)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?,
        )?;
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
            && input.joined_at.as_ref().is_none_or(|_| {
                before.joined_at.as_ref().is_some_and(|v| {
                    Some(v.seconds * 1_000_000 + i64::from(v.nanos / 1000)) == joined
                })
            })
            && input
                .discord_id
                .as_ref()
                .is_none_or(|v| Some(v) == before.discord_id.as_ref())
            && input
                .discord_username
                .as_ref()
                .is_none_or(|v| Some(v) == before.discord_username.as_ref())
        {
            return Ok(Outcome::Unchanged(MemberUnchanged {
                member: Some(before),
            }));
        }
        let sql = r#"
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
                id AS member_id,
                email,
                "firstName",
                "lastName",
                "discordId",
                "discordUsername",
                "joinedAt" AS joined,
                "updatedAt" AS updated,
                "archivedAt" AS archived
            "#;
        let stored = member(
            sqlx::query(sql)
                .bind(input.member_id)
                .bind(input.email)
                .bind(input.first_name)
                .bind(input.last_name)
                .bind(joined)
                .bind(input.discord_id)
                .bind(input.discord_username)
                .fetch_one(&mut **tx)
                .await?,
        )?;
        let event = MemberUpdated {
            changed_fields: Some(changed_fields(&before, &stored)),
            before: Some(before),
            after: Some(stored.clone()),
        };
        publish(tx, "MemberUpdated", event.encode_to_vec()).await?;
        Ok(Outcome::Updated(Box::new(event)))
    }

    pub async fn archive_member(&self, input: ArchiveMemberRequest) -> Result<Member, sqlx::Error> {
        match self.archive_operation(input).await? {
            Outcome::Archived(event) => Ok(event.member.expect("archived member")),
            Outcome::Unchanged(event) => Ok(event.member.expect("existing member")),
            _ => unreachable!(),
        }
    }

    pub async fn archive_operation(
        &self,
        input: ArchiveMemberRequest,
    ) -> Result<Outcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let sql = r#"
            UPDATE
                "Member"
            SET
                "archivedAt" = statement_timestamp(),
                "updatedAt" = statement_timestamp()
            WHERE
                id = $1
                AND "archivedAt" IS NULL
            RETURNING
                id AS member_id,
                email,
                "firstName",
                "lastName",
                "discordId",
                "discordUsername",
                "joinedAt" AS joined,
                "updatedAt" AS updated,
                "archivedAt" AS archived
            "#;
        let updated = sqlx::query(sql)
            .bind(input.member_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = updated else {
            let sql = r#"
                SELECT
                    id AS member_id,
                    email,
                    "firstName",
                    "lastName",
                    "discordId",
                    "discordUsername",
                    "joinedAt" AS joined,
                    "updatedAt" AS updated,
                    "archivedAt" AS archived
                FROM
                    "Member"
                WHERE
                    id = $1
                "#;
            let existing = member(
                sqlx::query(sql)
                    .bind(input.member_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or(sqlx::Error::RowNotFound)?,
            )?;
            tx.commit().await?;
            return Ok(Outcome::Unchanged(MemberUnchanged {
                member: Some(existing),
            }));
        };
        let stored = member(row)?;
        publish(
            &mut tx,
            "MemberArchived",
            MemberArchived {
                member: Some(stored.clone()),
            }
            .encode_to_vec(),
        )
        .await?;
        tx.commit().await?;
        Ok(Outcome::Archived(MemberArchived {
            member: Some(stored),
        }))
    }

    pub async fn upsert_member(
        &self,
        input: CreateMemberRequest,
        joined: Option<i64>,
    ) -> Result<Outcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        if let Some(stored) = Self::insert_member(&mut tx, input.clone(), joined, true).await? {
            tx.commit().await?;
            return Ok(Outcome::Created(MemberCreated {
                member: Some(stored),
            }));
        }
        let id = sqlx::query_scalar(
            r#"
            SELECT
                id
            FROM
                "Member"
            WHERE
                email = $1
            FOR UPDATE
            "#,
        )
        .bind(&input.email)
        .fetch_one(&mut *tx)
        .await?;
        let outcome = Self::update_in_transaction(
            &mut tx,
            UpdateMemberRequest {
                member_id: id,
                email: Some(input.email),
                first_name: Some(input.first_name),
                last_name: Some(input.last_name),
                joined_at: input.joined_at,
                discord_id: input.discord_id,
                discord_username: input.discord_username,
            },
            joined,
            true,
        )
        .await?;
        tx.commit().await?;
        Ok(outcome)
    }
}

fn member(row: sqlx::postgres::PgRow) -> Result<Member, sqlx::Error> {
    Ok(crate::db::members::Member {
        id: row.try_get("member_id")?,
        email: row.try_get("email")?,
        first_name: row.try_get("firstName")?,
        last_name: row.try_get("lastName")?,
        discord_id: row.try_get("discordId")?,
        discord_username: row.try_get("discordUsername")?,
        joined_at: row.try_get("joined")?,
        updated_at: row.try_get("updated")?,
        archived_at: row.try_get("archived")?,
    }
    .into())
}

fn changed_fields(before: &Member, after: &Member) -> prost_types::FieldMask {
    let fields = [
        ("email", before.email != after.email),
        ("first_name", before.first_name != after.first_name),
        ("last_name", before.last_name != after.last_name),
        ("joined_at", before.joined_at != after.joined_at),
        ("discord_id", before.discord_id != after.discord_id),
        (
            "discord_username",
            before.discord_username != after.discord_username,
        ),
        ("updated_at", before.updated_at != after.updated_at),
        ("archived_at", before.archived_at != after.archived_at),
    ];
    prost_types::FieldMask {
        paths: fields
            .into_iter()
            .filter(|(_, changed)| *changed)
            .map(|(field, _)| field.to_owned())
            .collect(),
    }
}

async fn publish(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    name: &str,
    payload: Vec<u8>,
) -> Result<(), sqlx::Error> {
    let id: i64 = sqlx::query_scalar(
        r#"
            INSERT INTO "MemberEvents"
                ("eventType", payload)
            VALUES
                ($1, $2)
            RETURNING
                id
            "#,
    )
    .bind(name)
    .bind(payload)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(name)
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
