use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
#[sqlx(rename_all = "camelCase")]
pub struct Member {
    pub id: Uuid,
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
    Id(Uuid),
    Email(String),
    DiscordId(String),
}

#[derive(Clone)]
pub struct MembersRepository {
    pool: PgPool,
}

const SELECT_MEMBER: &str = r#"
    SELECT id, email, "firstName", "lastName", "joinedAt",
           "discordId", "discordUsername", "updatedAt", "archivedAt"
    FROM "Member"
"#;

impl MembersRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, identifier: MemberIdentifier) -> Result<Option<Member>, sqlx::Error> {
        let mut query = QueryBuilder::<Postgres>::new(SELECT_MEMBER);
        match identifier {
            MemberIdentifier::Id(id) => query.push(" WHERE id = ").push_bind(id),
            MemberIdentifier::Email(email) => query.push(" WHERE email = ").push_bind(email),
            MemberIdentifier::DiscordId(id) => query.push(" WHERE \"discordId\" = ").push_bind(id),
        };
        query.build_query_as().fetch_optional(&self.pool).await
    }

    pub async fn list(
        &self,
        include_archived: bool,
        after: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<Member>, sqlx::Error> {
        let mut query = QueryBuilder::<Postgres>::new(SELECT_MEMBER);
        query
            .push(" WHERE (")
            .push_bind(include_archived)
            .push(" OR \"archivedAt\" IS NULL)");
        if let Some(id) = after {
            query.push(" AND id > ").push_bind(id);
        }
        query.push(" ORDER BY id ASC LIMIT ").push_bind(limit);
        query.build_query_as().fetch_all(&self.pool).await
    }
}
