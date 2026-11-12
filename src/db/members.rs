use std::collections::HashMap;

use chrono::Utc;
use sqlx::PgPool;

use crate::domain::members::{
    AutocompleteField, CommunicationMethod, CreateMember, Member, MemberIdentifier, MemberRole,
    OperationOutcome, UpdateMember, normalize_preferences,
};

struct MemberRow {
    id: i32,
    email: String,
    first_name: String,
    last_name: String,
    joined_at: chrono::DateTime<Utc>,
    discord_id: Option<String>,
    discord_username: Option<String>,
    linkedin_username: Option<String>,
    instagram_username: Option<String>,
    career: Option<String>,
    industry: Option<String>,
    profile: Option<String>,
    role: Option<MemberRole>,
    school: Option<String>,
    grad_date: Option<String>,
    updated_at: chrono::DateTime<Utc>,
    archived_at: Option<chrono::DateTime<Utc>>,
}

impl From<MemberRow> for Member {
    fn from(row: MemberRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            first_name: row.first_name,
            last_name: row.last_name,
            joined_at: row.joined_at,
            discord_id: row.discord_id,
            discord_username: row.discord_username,
            linkedin_username: row.linkedin_username,
            instagram_username: row.instagram_username,
            career: row.career,
            industry: row.industry,
            profile: row.profile,
            role: row.role,
            school: row.school,
            grad_date: row.grad_date,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
            preferred_communications: vec![],
            secondary_emails: vec![],
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

    /// Batch-loads stored preferences for `member_ids`, keyed by member id.
    /// Members without preferences are absent from the map.
    async fn load_preferences<'e, E>(
        &self,
        member_ids: &[i32],
        executor: E,
    ) -> Result<HashMap<i32, Vec<CommunicationMethod>>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut preferences = HashMap::<i32, Vec<CommunicationMethod>>::new();
        if member_ids.is_empty() {
            return Ok(preferences);
        }
        let rows = sqlx::query!(
            r#"
            SELECT
                member_id,
                method AS "method: CommunicationMethod"
            FROM
                user_preferred_communication
            WHERE
                member_id = ANY($1)
            ORDER BY
                member_id,
                method
            "#,
            member_ids,
        )
        .fetch_all(executor)
        .await?;
        for row in rows {
            preferences
                .entry(row.member_id)
                .or_default()
                .push(row.method);
        }
        Ok(preferences)
    }

    /// Loads stored preferences for a single member.
    async fn load_member_preferences<'e, E>(
        &self,
        member_id: i32,
        executor: E,
    ) -> Result<Vec<CommunicationMethod>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        Ok(self
            .load_preferences(&[member_id], executor)
            .await?
            .remove(&member_id)
            .unwrap_or_default())
    }

    /// Batch-loads non-primary emails for `member_ids`, keyed by member id.
    /// Members without secondary emails are absent from the map.
    async fn load_secondary_emails<'e, E>(
        &self,
        member_ids: &[i32],
        executor: E,
    ) -> Result<HashMap<i32, Vec<String>>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut emails = HashMap::<i32, Vec<String>>::new();
        if member_ids.is_empty() {
            return Ok(emails);
        }
        let rows = sqlx::query!(
            r#"
            SELECT
                "memberId" AS member_id,
                email
            FROM
                "UserEmails"
            WHERE
                "memberId" = ANY($1)
                AND NOT "isPrimary"
            ORDER BY
                "memberId",
                email COLLATE "C"
            "#,
            member_ids,
        )
        .fetch_all(executor)
        .await?;
        for row in rows {
            emails.entry(row.member_id).or_default().push(row.email);
        }
        Ok(emails)
    }

    /// Loads non-primary emails for a single member.
    async fn load_member_secondary_emails<'e, E>(
        &self,
        member_id: i32,
        executor: E,
    ) -> Result<Vec<String>, sqlx::Error>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        Ok(self
            .load_secondary_emails(&[member_id], executor)
            .await?
            .remove(&member_id)
            .unwrap_or_default())
    }

    async fn insert_secondary_emails(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        member_id: i32,
        emails: &[String],
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO "UserEmails"
                ("memberId", email, "isPrimary")
            SELECT
                $1,
                unnest($2::text[]),
                false
            "#,
            member_id,
            emails,
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn insert_primary_email(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        member_id: i32,
        email: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO "UserEmails"
                ("memberId", email, "isPrimary")
            VALUES
                ($1, $2, true)
            "#,
            member_id,
            email,
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn replace_preferences(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        member_id: i32,
        values: &[CommunicationMethod],
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            DELETE FROM
                user_preferred_communication
            WHERE
                member_id = $1
            "#,
            member_id
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query!(
            r#"
            INSERT INTO user_preferred_communication
                (member_id, method)
            SELECT
                $1,
                unnest($2::communication_method[])
            "#,
            member_id,
            values as &[CommunicationMethod],
        )
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn update_in_transaction(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        input: UpdateMember,
        skip_unchanged: bool,
    ) -> Result<UpdateResult, sqlx::Error> {
        let mut before: Member = sqlx::query_as!(
            MemberRow,
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
                school,
                grad_date,
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
        .await?
        .into();
        before.preferred_communications =
            self.load_member_preferences(before.id, &mut **tx).await?;
        before.secondary_emails = self
            .load_member_secondary_emails(before.id, &mut **tx)
            .await?;

        let after = input.apply(&before)?;

        if skip_unchanged && after == before {
            return Ok(UpdateResult {
                before,
                after,
                changed: false,
            });
        }

        let preferences = after.preferred_communications.clone();
        let secondary_emails = after.secondary_emails.clone();
        let mut after: Member = sqlx::query_as!(
            MemberRow,
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
                "updatedAt" = $14,
                school = $15,
                grad_date = $16
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
                school,
                grad_date,
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
            after.role as Option<MemberRole>,
            Utc::now(),
            after.school,
            after.grad_date
        )
        .fetch_one(&mut **tx)
        .await?
        .into();
        if preferences != before.preferred_communications {
            self.replace_preferences(tx, before.id, &preferences)
                .await?;
        }
        after.preferred_communications = preferences;

        // UNIQUE(email) forces this order: drop old secondaries, swap the primary,
        // then insert new secondaries. Covers promoting a secondary to primary and
        // demoting the primary to a secondary in one request.
        let secondaries_changed = secondary_emails != before.secondary_emails;
        if secondaries_changed {
            sqlx::query!(
                r#"
                DELETE FROM
                    "UserEmails"
                WHERE
                    "memberId" = $1
                    AND NOT "isPrimary"
                "#,
                before.id
            )
            .execute(&mut **tx)
            .await?;
        }
        if after.email != before.email {
            sqlx::query!(
                r#"
                DELETE FROM
                    "UserEmails"
                WHERE
                    "memberId" = $1
                    AND "isPrimary"
                "#,
                before.id
            )
            .execute(&mut **tx)
            .await?;
            self.insert_primary_email(tx, before.id, &after.email)
                .await?;
        }
        if secondaries_changed && !secondary_emails.is_empty() {
            self.insert_secondary_emails(tx, before.id, &secondary_emails)
                .await?;
        }
        after.secondary_emails = secondary_emails;

        Ok(UpdateResult {
            before,
            after,
            changed: true,
        })
    }

    async fn archive(&self, member_id: i32) -> Result<(Member, bool), sqlx::Error> {
        let updated = sqlx::query_as!(
            MemberRow,
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
                school,
                grad_date,
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
                MemberRow,
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
                    school,
                    grad_date,
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

            let mut existing: Member = existing.into();
            existing.preferred_communications = self
                .load_member_preferences(existing.id, &self.pool)
                .await?;
            existing.secondary_emails = self
                .load_member_secondary_emails(existing.id, &self.pool)
                .await?;
            return Ok((existing, false));
        };

        let mut stored: Member = stored.into();
        stored.preferred_communications =
            self.load_member_preferences(stored.id, &self.pool).await?;
        stored.secondary_emails = self
            .load_member_secondary_emails(stored.id, &self.pool)
            .await?;
        Ok((stored, true))
    }
}

impl MembersRepository for PgMembersRepository {
    async fn get(&self, identifier: MemberIdentifier) -> Result<Option<Member>, sqlx::Error> {
        let row = match identifier {
            MemberIdentifier::Id(id) => {
                sqlx::query_as!(
                    MemberRow,
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
                        school,
                        grad_date,
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
                    MemberRow,
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
                        school,
                        grad_date,
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
                    MemberRow,
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
                        school,
                        grad_date,
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
        }?;
        let mut member = row.map(Member::from);
        if let Some(member) = &mut member {
            member.preferred_communications =
                self.load_member_preferences(member.id, &self.pool).await?;
            member.secondary_emails = self
                .load_member_secondary_emails(member.id, &self.pool)
                .await?;
        }
        Ok(member)
    }

    async fn list(
        &self,
        include_archived: bool,
        after: Option<i32>,
        limit: i64,
    ) -> Result<Vec<Member>, sqlx::Error> {
        let rows = sqlx::query_as!(
            MemberRow,
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
                school,
                grad_date,
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
        .await?;
        let mut members: Vec<Member> = rows.into_iter().map(Into::into).collect();
        let ids: Vec<i32> = members.iter().map(|member| member.id).collect();
        let mut preferences = self.load_preferences(&ids, &self.pool).await?;
        let mut secondary_emails = self.load_secondary_emails(&ids, &self.pool).await?;
        for member in &mut members {
            member.preferred_communications = preferences.remove(&member.id).unwrap_or_default();
            member.secondary_emails = secondary_emails.remove(&member.id).unwrap_or_default();
        }
        Ok(members)
    }

    async fn create_member(&self, input: CreateMember) -> Result<Member, sqlx::Error> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        let preferences = normalize_preferences(input.preferred_communications.unwrap_or_default());
        let mut secondary_emails = input.secondary_emails.unwrap_or_default();
        secondary_emails.sort();

        let mut member: Member = sqlx::query_as!(
            MemberRow,
            r#"
            INSERT INTO "Member"
                (email, "firstName", "lastName", "joinedAt", "discordId", "discordUsername",
                 "linkedinUsername", "instagramUsername", career, industry, profile, role, "updatedAt", school, grad_date)
            VALUES
                ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
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
                school,
                grad_date,
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
            input.role as Option<MemberRole>,
            now,
            input.school,
            input.grad_date
        )
        .fetch_one(&mut *tx)
        .await?
        .into();
        if !preferences.is_empty() {
            self.replace_preferences(&mut tx, member.id, &preferences)
                .await?;
        }
        member.preferred_communications = preferences;
        self.insert_primary_email(&mut tx, member.id, &member.email)
            .await?;
        if !secondary_emails.is_empty() {
            self.insert_secondary_emails(&mut tx, member.id, &secondary_emails)
                .await?;
        }
        member.secondary_emails = secondary_emails;
        tx.commit().await?;
        Ok(member)
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
            AutocompleteField::School => {
                sqlx::query_scalar!(
                    r#"
                    SELECT
                        school AS "school!"
                    FROM
                        "Member"
                    WHERE
                        school IS NOT NULL
                        AND school ILIKE $1
                    GROUP BY
                        school
                    ORDER BY
                        COUNT(*) DESC,
                        school ASC
                    LIMIT
                        $2
                    "#,
                    pattern,
                    limit
                )
                .fetch_all(&self.pool)
                .await
            }
            AutocompleteField::GradDate => {
                sqlx::query_scalar!(
                    r#"
                    SELECT
                        grad_date AS "grad_date!"
                    FROM
                        "Member"
                    WHERE
                        grad_date IS NOT NULL
                        AND grad_date ILIKE $1
                    GROUP BY
                        grad_date
                    ORDER BY
                        COUNT(*) DESC,
                        grad_date ASC
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
                    school: input.school,
                    grad_date: input.grad_date,
                    profile: input.profile,
                    role: input.role,
                    preferred_communications: normalize_preferences(
                        input.preferred_communications.unwrap_or_default(),
                    ),
                    updated_at: DateTime::UNIX_EPOCH,
                    archived_at: None,
                    secondary_emails: input.secondary_emails.unwrap_or_default(),
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
                    AutocompleteField::School => member.school.as_deref(),
                    AutocompleteField::GradDate => member.grad_date.as_deref(),
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
