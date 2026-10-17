use prost::Message;
use sqlx::{PgPool, Row};

use crate::proto::members::{
    Member,
    admin::{
        ArchiveMemberRequest, CreateMemberRequest, MemberArchived, MemberCreated, MemberUpdated,
        UpdateMemberRequest,
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
        let sql = format!(
            r#"INSERT INTO "Member" (id, email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername", "updatedAt")
            VALUES (gen_random_uuid(), $1, $2, $3, COALESCE((to_timestamp(($4::bigint / 1000000)::double precision) + ($4::bigint % 1000000) * interval '1 microsecond'), CURRENT_TIMESTAMP), $5, $6, CURRENT_TIMESTAMP) RETURNING {RETURNING}"#
        );
        let stored = member(
            sqlx::query(&sql)
                .bind(input.email)
                .bind(input.first_name)
                .bind(input.last_name)
                .bind(joined)
                .bind(input.discord_id)
                .bind(input.discord_username)
                .fetch_one(&mut *tx)
                .await?,
        )?;
        publish(
            &mut tx,
            "MemberCreated",
            MemberCreated {
                member: Some(stored.clone()),
            }
            .encode_to_vec(),
        )
        .await?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn update_member(
        &self,
        input: UpdateMemberRequest,
        joined: Option<i64>,
    ) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let sql = format!(r#"SELECT {RETURNING} FROM "Member" WHERE id = $1::uuid FOR UPDATE"#);
        let before = member(
            sqlx::query(&sql)
                .bind(&input.member_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?,
        )?;
        let sql = format!(
            r#"UPDATE "Member" SET email = COALESCE($2, email), "firstName" = COALESCE($3, "firstName"),
            "lastName" = COALESCE($4, "lastName"), "joinedAt" = COALESCE((to_timestamp(($5::bigint / 1000000)::double precision) + ($5::bigint % 1000000) * interval '1 microsecond'), "joinedAt"),
            "discordId" = COALESCE($6, "discordId"), "discordUsername" = COALESCE($7, "discordUsername"),
            "updatedAt" = clock_timestamp() WHERE id = $1::uuid RETURNING {RETURNING}"#
        );
        let stored = member(
            sqlx::query(&sql)
                .bind(&input.member_id)
                .bind(input.email)
                .bind(input.first_name)
                .bind(input.last_name)
                .bind(joined)
                .bind(input.discord_id)
                .bind(input.discord_username)
                .fetch_one(&mut *tx)
                .await?,
        )?;
        let event = MemberUpdated {
            changed_fields: Some(changed_fields(&before, &stored)),
            before: Some(before),
            after: Some(stored.clone()),
        };
        publish(&mut tx, "MemberUpdated", event.encode_to_vec()).await?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn archive_member(&self, input: ArchiveMemberRequest) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let sql = format!(
            r#"UPDATE "Member" SET "archivedAt" = statement_timestamp(), "updatedAt" = statement_timestamp()
            WHERE id = $1::uuid RETURNING {RETURNING}"#
        );
        let stored = member(
            sqlx::query(&sql)
                .bind(input.member_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?,
        )?;
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
        Ok(stored)
    }
}

const RETURNING: &str = r#"id::text AS member_id, email, "firstName", "lastName", "discordId", "discordUsername",
    (extract(epoch FROM "joinedAt") * 1000000)::bigint AS joined,
    (extract(epoch FROM "updatedAt") * 1000000)::bigint AS updated,
    (extract(epoch FROM "archivedAt") * 1000000)::bigint AS archived"#;

fn member(row: sqlx::postgres::PgRow) -> Result<Member, sqlx::Error> {
    fn ts(micros: i64) -> prost_types::Timestamp {
        prost_types::Timestamp {
            seconds: micros.div_euclid(1_000_000),
            nanos: (micros.rem_euclid(1_000_000) * 1000) as i32,
        }
    }
    Ok(Member {
        member_id: row.try_get("member_id")?,
        email: row.try_get("email")?,
        first_name: row.try_get("firstName")?,
        last_name: row.try_get("lastName")?,
        discord_id: row.try_get("discordId")?,
        discord_username: row.try_get("discordUsername")?,
        joined_at: Some(ts(row.try_get("joined")?)),
        updated_at: Some(ts(row.try_get("updated")?)),
        archived_at: row.try_get::<Option<i64>, _>("archived")?.map(ts),
    })
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
        "INSERT INTO member_events (event_type, payload) VALUES ($1, $2) RETURNING id",
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
