use jiff::Timestamp;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub member_id: Uuid,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: Timestamp,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub updated_at: Timestamp,
    pub archived_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMember {
    pub member_id: Uuid,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: Timestamp,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub updated_at: Timestamp,
}
