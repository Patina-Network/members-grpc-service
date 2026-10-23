use chrono::{DateTime, Utc};

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

impl UpdateMember {
    /// Merges into `before`. Discord is checked only when touched, so a row with
    /// one half already set still accepts other edits.
    pub fn apply(self, before: &Member) -> Result<Member, sqlx::Error> {
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

pub enum UpsertWrite {
    Create(CreateMember),
    Update(UpdateMember),
}

impl UpsertWrite {
    /// matches on email.
    /// Discord fields (`discord_id`, `discord_username`) left out of the request keep their stored values.
    pub fn new(member: CreateMember, existing_id: Option<i32>) -> Self {
        let Some(member_id) = existing_id else {
            return Self::Create(member);
        };

        Self::Update(UpdateMember {
            member_id,
            email: Some(member.email),
            first_name: Some(member.first_name),
            last_name: Some(member.last_name),
            joined_at: member.joined_at,
            discord_id: member.discord_id.map(Some),
            discord_username: member.discord_username.map(Some),
        })
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

    fn create(discord: Option<(&str, &str)>) -> CreateMember {
        CreateMember {
            email: "a@example.org".into(),
            first_name: "First".into(),
            last_name: "Last".into(),
            joined_at: None,
            discord_id: discord.map(|(id, _)| id.into()),
            discord_username: discord.map(|(_, name)| name.into()),
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

    /// `update_operation` skips the write and reports `Unchanged` when `apply` returns the stored row.
    #[test]
    fn rewriting_stored_values_is_unchanged() {
        let before = member(Some(("1", "name")));
        let rewrite = |email: &str| UpdateMember {
            email: Some(email.into()),
            first_name: Some("First".into()),
            last_name: Some("Last".into()),
            joined_at: Some(DateTime::UNIX_EPOCH),
            ..update(Some(Some("1")), Some(Some("name")))
        };

        assert_eq!(
            rewrite("a@example.org").apply(&before).ok(),
            Some(before.clone())
        );
        assert_ne!(rewrite("b@example.org").apply(&before).ok(), Some(before));
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

    #[test]
    fn upsert_creates_new_members_and_updates_existing_ones_by_id() {
        assert!(matches!(
            &UpsertWrite::new(create(None), None),
            UpsertWrite::Create(CreateMember { email, .. }) if email == "a@example.org"
        ));

        assert!(matches!(
            &UpsertWrite::new(create(None), Some(7)),
            UpsertWrite::Update(UpdateMember {
                member_id: 7,
                email: Some(email),
                first_name: Some(first_name),
                last_name: Some(last_name),
                joined_at: None,
                discord_id: None,
                discord_username: None,
            }) if email == "a@example.org" && first_name == "First" && last_name == "Last"
        ));

        assert!(matches!(
            &UpsertWrite::new(create(Some(("42", "name"))), Some(7)),
            UpsertWrite::Update(UpdateMember {
                discord_id: Some(Some(id)),
                discord_username: Some(Some(name)),
                ..
            }) if id == "42" && name == "name"
        ));
    }
}
