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
    /// `None` = dont change, `Some(None)` clears it
    pub discord_id: Option<Option<String>>,
    /// `None` = dont change, `Some(None)` clears it
    pub discord_username: Option<Option<String>>,
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

        let after = input.apply(&before)?;

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
            before.id,
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

impl UpdateMember {
    /// Merges into `before`. Discord is checked only when touched, so a row with
    /// one half already set still accepts other edits.
    fn apply(self, before: &Member) -> Result<Member, sqlx::Error> {
        let mut after = before.clone();
        if let Some(email) = self.email {
            after.email = email;
        }
        if let Some(first_name) = self.first_name {
            after.first_name = first_name;
        }
        if let Some(last_name) = self.last_name {
            after.last_name = last_name;
        }
        if let Some(joined_at) = self.joined_at {
            after.joined_at = joined_at;
        }

        let discord_touched = self.discord_id.is_some() || self.discord_username.is_some();
        if let Some(discord_id) = self.discord_id {
            after.discord_id = discord_id;
        }
        if let Some(discord_username) = self.discord_username {
            after.discord_username = discord_username;
        }
        if discord_touched && after.discord_id.is_some() != after.discord_username.is_some() {
            return Err(sqlx::Error::InvalidArgument(
                "discord_id and discord_username must be set or cleared together".into(),
            ));
        }

        Ok(after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(discord: Option<(&str, &str)>) -> Member {
        Member {
            id: 1,
            email: "a@example.org".into(),
            first_name: "First".into(),
            last_name: "Last".into(),
            joined_at: DateTime::UNIX_EPOCH,
            discord_id: discord.map(|(id, _)| id.into()),
            discord_username: discord.map(|(_, name)| name.into()),
            updated_at: DateTime::UNIX_EPOCH,
            archived_at: None,
        }
    }

    fn update(
        discord_id: Option<Option<&str>>,
        discord_username: Option<Option<&str>>,
    ) -> UpdateMember {
        UpdateMember {
            member_id: 1,
            email: None,
            first_name: None,
            last_name: None,
            joined_at: None,
            discord_id: discord_id.map(|v| v.map(str::to_owned)),
            discord_username: discord_username.map(|v| v.map(str::to_owned)),
        }
    }

    fn discord(member: &Member) -> (Option<&str>, Option<&str>) {
        (
            member.discord_id.as_deref(),
            member.discord_username.as_deref(),
        )
    }

    #[test]
    fn discord_id_and_username_change_together() {
        let linked = member(Some(("1", "name")));
        let unlinked = member(None);

        for (before, input, expected) in [
            (&linked, update(Some(None), Some(None)), (None, None)),
            (
                &linked,
                update(Some(Some("2")), None),
                (Some("2"), Some("name")),
            ),
            (
                &linked,
                update(None, Some(Some("new"))),
                (Some("1"), Some("new")),
            ),
            (
                &unlinked,
                update(Some(Some("2")), Some(Some("n"))),
                (Some("2"), Some("n")),
            ),
        ] {
            assert_eq!(discord(&input.apply(before).unwrap()), expected);
        }

        for (before, input) in [
            (&linked, update(Some(None), None)),
            (&linked, update(None, Some(None))),
            (&linked, update(Some(None), Some(Some("still-here")))),
            (&unlinked, update(None, Some(Some("orphan")))),
            (&unlinked, update(Some(Some("2")), None)),
        ] {
            let error = input.apply(before).unwrap_err();
            assert!(matches!(error, sqlx::Error::InvalidArgument(_)), "{error}");
        }
    }

    #[test]
    fn untouched_discord_does_not_block_other_edits_on_half_set_rows() {
        let mut half_set = member(Some(("1", "name")));
        half_set.discord_username = None;

        let after = UpdateMember {
            first_name: Some("New".into()),
            ..update(None, None)
        }
        .apply(&half_set)
        .unwrap();

        assert_eq!(after.first_name, "New");
        assert_eq!(discord(&after), (Some("1"), None));
    }
}
