use chrono::Utc;
use sqlx::PgPool;

use crate::domain::members::{
    AutocompleteField, CreateMember, Member, MemberIdentifier, MemberRole, OperationOutcome,
    UpdateMember,
};

impl MemberRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Board => "board",
            Self::Mentee => "mentee",
            Self::Mentor => "mentor",
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for MemberRole {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <String as sqlx::Type<sqlx::Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for MemberRole {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        match value {
            "board" => Ok(Self::Board),
            "mentee" => Ok(Self::Mentee),
            "mentor" => Ok(Self::Mentor),
            _ => Err(format!("invalid member role: {value}").into()),
        }
    }
}

pub trait MembersRepository: Send + Sync + 'static {
    fn get(
        &self,
        identifier: MemberIdentifier,
    ) -> impl Future<Output = Result<Option<Member>, sqlx::Error>> + Send;

    fn list(
        &self,
        include_archived: bool,
        after: Option<i32>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Member>, sqlx::Error>> + Send;

    fn create_member(
        &self,
        input: CreateMember,
    ) -> impl Future<Output = Result<Member, sqlx::Error>> + Send;

    fn update_member(
        &self,
        input: UpdateMember,
    ) -> impl Future<Output = Result<Member, sqlx::Error>> + Send;

    fn update_operation(
        &self,
        input: UpdateMember,
    ) -> impl Future<Output = Result<OperationOutcome, sqlx::Error>> + Send;

    fn archive_member(
        &self,
        member_id: i32,
    ) -> impl Future<Output = Result<Member, sqlx::Error>> + Send;

    fn archive_operation(
        &self,
        member_id: i32,
    ) -> impl Future<Output = Result<OperationOutcome, sqlx::Error>> + Send;

    fn autocomplete(
        &self,
        field: AutocompleteField,
        query: String,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<String>, sqlx::Error>> + Send;
}

#[derive(Clone)]
pub struct PgMembersRepository {
    pool: PgPool,
}

impl PgMembersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
                "linkedinUsername" AS linkedin_username,
                "instagramUsername" AS instagram_username,
                career,
                industry,
                profile,
                role AS "role: MemberRole",
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
                "linkedinUsername" = $8,
                "instagramUsername" = $9,
                career = $10,
                industry = $11,
                profile = $12,
                role = $13,
                "updatedAt" = $14
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
                "linkedinUsername" AS linkedin_username,
                "instagramUsername" AS instagram_username,
                career,
                industry,
                profile,
                role AS "role: MemberRole",
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
            after.linkedin_username,
            after.instagram_username,
            after.career,
            after.industry,
            after.profile,
            after.role.map(MemberRole::as_str),
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
                "linkedinUsername" AS linkedin_username,
                "instagramUsername" AS instagram_username,
                career,
                industry,
                profile,
                role AS "role: MemberRole",
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
                    "linkedinUsername" AS linkedin_username,
                    "instagramUsername" AS instagram_username,
                    career,
                    industry,
                    profile,
                    role AS "role: MemberRole",
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

impl MembersRepository for PgMembersRepository {
    async fn get(&self, identifier: MemberIdentifier) -> Result<Option<Member>, sqlx::Error> {
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
                        "linkedinUsername" AS linkedin_username,
                        "instagramUsername" AS instagram_username,
                        career,
                        industry,
                        profile,
                        role AS "role: MemberRole",
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
                        "linkedinUsername" AS linkedin_username,
                        "instagramUsername" AS instagram_username,
                        career,
                        industry,
                        profile,
                        role AS "role: MemberRole",
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
                        "linkedinUsername" AS linkedin_username,
                        "instagramUsername" AS instagram_username,
                        career,
                        industry,
                        profile,
                        role AS "role: MemberRole",
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

    async fn list(
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
                "linkedinUsername" AS linkedin_username,
                "instagramUsername" AS instagram_username,
                career,
                industry,
                profile,
                role AS "role: MemberRole",
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

    async fn create_member(&self, input: CreateMember) -> Result<Member, sqlx::Error> {
        let now = Utc::now();

        sqlx::query_as!(
            Member,
            r#"
            INSERT INTO "Member"
                (email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername",
                 "linkedinUsername", "instagramUsername", career, industry, profile, role, "updatedAt")
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING
                id,
                email,
                "firstName" AS first_name,
                "lastName" AS last_name,
                "joinedAt" AS joined_at,
                "discordId" AS discord_id,
                "discordUsername" AS discord_username,
                "linkedinUsername" AS linkedin_username,
                "instagramUsername" AS instagram_username,
                career,
                industry,
                profile,
                role AS "role: MemberRole",
                "updatedAt" AS updated_at,
                "archivedAt" AS archived_at
            "#,
            input.email,
            input.first_name,
            input.last_name,
            input.joined_at.unwrap_or(now),
            input.discord_id,
            input.discord_username,
            input.linkedin_username,
            input.instagram_username,
            input.career,
            input.industry,
            input.profile,
            input.role.map(MemberRole::as_str),
            now
        )
        .fetch_one(&self.pool)
        .await
    }

    async fn update_member(&self, input: UpdateMember) -> Result<Member, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = self.update_in_transaction(&mut tx, input, false).await?;
        tx.commit().await?;

        Ok(result.after)
    }

    async fn update_operation(&self, input: UpdateMember) -> Result<OperationOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let result = self.update_in_transaction(&mut tx, input, true).await?;
        tx.commit().await?;

        Ok(result.into_outcome())
    }

    async fn archive_member(&self, member_id: i32) -> Result<Member, sqlx::Error> {
        Ok(self.archive(member_id).await?.0)
    }

    async fn archive_operation(&self, member_id: i32) -> Result<OperationOutcome, sqlx::Error> {
        let (member, changed) = self.archive(member_id).await?;
        Ok(if changed {
            OperationOutcome::Archived(member)
        } else {
            OperationOutcome::Unchanged(member)
        })
    }

    async fn autocomplete(
        &self,
        field: AutocompleteField,
        query: String,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let pattern = format!("%{query}%");

        match field {
            AutocompleteField::Career => {
                sqlx::query_scalar!(
                    r#"
                    SELECT
                        career AS "career!"
                    FROM
                        "Member"
                    WHERE
                        career IS NOT NULL
                        AND career ILIKE $1
                    GROUP BY
                        career
                    ORDER BY
                        COUNT(*) DESC,
                        career ASC
                    LIMIT
                        $2
                    "#,
                    pattern,
                    limit
                )
                .fetch_all(&self.pool)
                .await
            }
            AutocompleteField::Industry => {
                sqlx::query_scalar!(
                    r#"
                    SELECT
                        industry AS "industry!"
                    FROM
                        "Member"
                    WHERE
                        industry IS NOT NULL
                        AND industry ILIKE $1
                    GROUP BY
                        industry
                    ORDER BY
                        COUNT(*) DESC,
                        industry ASC
                    LIMIT
                        $2
                    "#,
                    pattern,
                    limit
                )
                .fetch_all(&self.pool)
                .await
            }
        }
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
                before: Box::new(self.before),
                after: Box::new(self.after),
            }
        } else {
            OperationOutcome::Unchanged(self.after)
        }
    }
}

#[cfg(test)]
pub mod fake {
    use std::collections::BTreeMap;

    use chrono::DateTime;

    use super::*;

    #[derive(Clone)]
    pub enum FakeMembersRepository {
        Unavailable,
        Hang,
        Stored(Vec<Member>),
    }

    impl FakeMembersRepository {
        async fn with<T>(
            &self,
            f: impl FnOnce(&[Member]) -> Result<T, sqlx::Error>,
        ) -> Result<T, sqlx::Error> {
            match self {
                Self::Unavailable => Err(sqlx::Error::PoolClosed),
                Self::Hang => std::future::pending().await,
                Self::Stored(members) => f(members),
            }
        }
    }

    fn find(members: &[Member], id: i32) -> Result<Member, sqlx::Error> {
        members
            .iter()
            .find(|member| member.id == id)
            .cloned()
            .ok_or(sqlx::Error::RowNotFound)
    }

    fn archive(members: &[Member], id: i32) -> Result<(Member, bool), sqlx::Error> {
        let mut member = find(members, id)?;
        let changed = member.archived_at.is_none();
        member.archived_at.get_or_insert(DateTime::UNIX_EPOCH);
        Ok((member, changed))
    }

    impl MembersRepository for FakeMembersRepository {
        async fn get(&self, identifier: MemberIdentifier) -> Result<Option<Member>, sqlx::Error> {
            self.with(|members| {
                Ok(members
                    .iter()
                    .find(|member| match &identifier {
                        MemberIdentifier::Id(id) => member.id == *id,
                        MemberIdentifier::Email(email) => &member.email == email,
                        MemberIdentifier::DiscordId(id) => member.discord_id.as_ref() == Some(id),
                    })
                    .cloned())
            })
            .await
        }

        async fn list(
            &self,
            include_archived: bool,
            after: Option<i32>,
            limit: i64,
        ) -> Result<Vec<Member>, sqlx::Error> {
            self.with(|members| {
                Ok(members
                    .iter()
                    .filter(|member| include_archived || member.archived_at.is_none())
                    .filter(|member| after.is_none_or(|after| member.id > after))
                    .take(limit as usize)
                    .cloned()
                    .collect())
            })
            .await
        }

        async fn create_member(&self, input: CreateMember) -> Result<Member, sqlx::Error> {
            self.with(|members| {
                Ok(Member {
                    id: members.len() as i32 + 1,
                    email: input.email,
                    first_name: input.first_name,
                    last_name: input.last_name,
                    joined_at: input.joined_at.unwrap_or(DateTime::UNIX_EPOCH),
                    discord_id: input.discord_id,
                    discord_username: input.discord_username,
                    linkedin_username: input.linkedin_username,
                    instagram_username: input.instagram_username,
                    career: input.career,
                    industry: input.industry,
                    profile: input.profile,
                    role: input.role,
                    updated_at: DateTime::UNIX_EPOCH,
                    archived_at: None,
                })
            })
            .await
        }

        async fn update_member(&self, input: UpdateMember) -> Result<Member, sqlx::Error> {
            self.with(|members| {
                let before = find(members, input.member_id)?;
                input.apply(&before)
            })
            .await
        }

        async fn update_operation(
            &self,
            input: UpdateMember,
        ) -> Result<OperationOutcome, sqlx::Error> {
            self.with(|members| {
                let before = find(members, input.member_id)?;
                let after = input.apply(&before)?;
                Ok(if after == before {
                    OperationOutcome::Unchanged(after)
                } else {
                    OperationOutcome::Updated {
                        before: Box::new(before),
                        after: Box::new(after),
                    }
                })
            })
            .await
        }

        async fn archive_member(&self, member_id: i32) -> Result<Member, sqlx::Error> {
            self.with(|members| archive(members, member_id).map(|(member, _)| member))
                .await
        }

        async fn archive_operation(&self, member_id: i32) -> Result<OperationOutcome, sqlx::Error> {
            self.with(|members| {
                let (member, changed) = archive(members, member_id)?;
                Ok(if changed {
                    OperationOutcome::Archived(member)
                } else {
                    OperationOutcome::Unchanged(member)
                })
            })
            .await
        }

        async fn autocomplete(
            &self,
            field: AutocompleteField,
            query: String,
            limit: i64,
        ) -> Result<Vec<String>, sqlx::Error> {
            self.with(|members| {
                let query = query.to_lowercase();
                let mut counts = BTreeMap::<&str, usize>::new();
                for value in members.iter().filter_map(|member| match field {
                    AutocompleteField::Career => member.career.as_deref(),
                    AutocompleteField::Industry => member.industry.as_deref(),
                }) {
                    if value.to_lowercase().contains(&query) {
                        *counts.entry(value).or_default() += 1;
                    }
                }

                let mut values: Vec<_> = counts.into_iter().collect();
                values.sort_by(|(a, a_count), (b, b_count)| b_count.cmp(a_count).then(a.cmp(b)));
                Ok(values
                    .into_iter()
                    .take(limit as usize)
                    .map(|(value, _)| value.to_owned())
                    .collect())
            })
            .await
        }
    }
}
