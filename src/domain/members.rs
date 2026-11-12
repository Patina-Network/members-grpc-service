use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "\"MemberRole\"", rename_all = "lowercase")]
pub enum MemberRole {
    Board,
    Mentee,
    Mentor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, sqlx::Type)]
#[sqlx(type_name = "communication_method")]
pub enum CommunicationMethod {
    Email,
    PhoneNumber,
    Discord,
    Instagram,
    LinkedIn,
}

/// Canonical ordering makes preference equality independent of request order.
pub fn normalize_preferences(mut values: Vec<CommunicationMethod>) -> Vec<CommunicationMethod> {
    values.sort_unstable();
    values.dedup();
    values
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub id: i32,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: DateTime<Utc>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub linkedin_username: Option<String>,
    pub instagram_username: Option<String>,
    pub career: Option<String>,
    pub industry: Option<String>,
    pub school: Option<String>,
    pub grad_date: Option<String>,
    pub profile: Option<String>,
    pub role: Option<MemberRole>,
    pub preferred_communications: Vec<CommunicationMethod>,
    pub secondary_emails: Vec<String>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

pub enum MemberIdentifier {
    Id(i32),
    Email(String),
    DiscordId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocompleteField {
    Career,
    Industry,
    School,
    GradDate,
}

pub struct CreateMember {
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub joined_at: Option<DateTime<Utc>>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub linkedin_username: Option<String>,
    pub instagram_username: Option<String>,
    pub career: Option<String>,
    pub industry: Option<String>,
    pub school: Option<String>,
    pub grad_date: Option<String>,
    pub profile: Option<String>,
    pub role: Option<MemberRole>,
    pub preferred_communications: Option<Vec<CommunicationMethod>>,
    pub secondary_emails: Option<Vec<String>>,
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
    /// `None` = dont change, `Some(None)` clears it
    pub linkedin_username: Option<Option<String>>,
    /// `None` = dont change, `Some(None)` clears it
    pub instagram_username: Option<Option<String>>,
    /// `None` = dont change, `Some(None)` clears it
    pub career: Option<Option<String>>,
    /// `None` = dont change, `Some(None)` clears it
    pub industry: Option<Option<String>>,
    pub school: Option<Option<String>>,
    pub grad_date: Option<Option<String>>,
    /// `None` = dont change, `Some(None)` clears it
    pub profile: Option<Option<String>>,
    /// `None` = dont change, `Some(None)` clears it
    pub role: Option<Option<MemberRole>>,
    /// None = unchanged; Some(empty) clears all preferences.
    pub preferred_communications: Option<Vec<CommunicationMethod>>,
    /// None = unchanged; Some(empty) clears all secondary emails.
    pub secondary_emails: Option<Vec<String>>,
}

#[derive(Clone)]
pub enum OperationOutcome {
    Created(Member),
    Updated {
        before: Box<Member>,
        after: Box<Member>,
    },
    Archived(Member),
    Unchanged(Member),
}

impl UpdateMember {
    /// Merges into `before`. Discord is checked only when touched, so a row with
    /// one half already set still accepts other edits.
    pub fn apply(self, before: &Member) -> Result<Member, sqlx::Error> {
        let mut after = before.clone();

        if let Some(first_name) = self.first_name {
            after.first_name = first_name;
        }
        if let Some(last_name) = self.last_name {
            after.last_name = last_name;
        }
        if let Some(joined_at) = self.joined_at {
            after.joined_at = joined_at;
        }
        if let Some(linkedin_username) = self.linkedin_username {
            after.linkedin_username = linkedin_username;
        }
        if let Some(instagram_username) = self.instagram_username {
            after.instagram_username = instagram_username;
        }
        if let Some(career) = self.career {
            after.career = career;
        }
        if let Some(industry) = self.industry {
            after.industry = industry;
        }
        if let Some(school) = self.school {
            after.school = school;
        }
        if let Some(grad_date) = self.grad_date {
            after.grad_date = grad_date;
        }
        if let Some(profile) = self.profile {
            after.profile = profile;
        }
        if let Some(role) = self.role {
            after.role = role;
        }

        if let Some(values) = self.preferred_communications {
            after.preferred_communications = normalize_preferences(values);
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

        let emails_touched = self.email.is_some() || self.secondary_emails.is_some();
        if let Some(email) = self.email {
            after.email = email;
        }
        if let Some(secondary_emails) = self.secondary_emails {
            after.secondary_emails = secondary_emails;
        }
        if emails_touched && after.secondary_emails.contains(&after.email) {
            return Err(sqlx::Error::InvalidArgument(
                "secondary_emails cannot contain the primary email".into(),
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
            linkedin_username: member.linkedin_username.map(Some),
            instagram_username: member.instagram_username.map(Some),
            career: member.career.map(Some),
            industry: member.industry.map(Some),
            school: member.school.map(Some),
            grad_date: member.grad_date.map(Some),
            profile: member.profile.map(Some),
            role: member.role.map(Some),
            preferred_communications: member.preferred_communications,
            secondary_emails: member.secondary_emails,
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
            linkedin_username: None,
            instagram_username: None,
            career: None,
            industry: None,
            school: None,
            grad_date: None,
            profile: None,
            role: None,
            preferred_communications: vec![],
            secondary_emails: vec![],
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
            linkedin_username: None,
            instagram_username: None,
            career: None,
            industry: None,
            school: None,
            grad_date: None,
            profile: None,
            role: None,
            preferred_communications: None,
            secondary_emails: None,
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
            linkedin_username: None,
            instagram_username: None,
            career: None,
            industry: None,
            school: None,
            grad_date: None,
            profile: None,
            role: None,
            preferred_communications: None,
            secondary_emails: None,
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
    fn optional_fields_set_clear_or_keep_independently() {
        let mut before = member(None);
        before.linkedin_username = Some("old".into());
        before.career = Some("Engineer".into());
        before.profile = Some("notes".into());
        before.role = Some(MemberRole::Mentee);

        let after = UpdateMember {
            linkedin_username: Some(None),
            instagram_username: Some(Some("insta".into())),
            profile: Some(Some("new notes".into())),
            role: Some(None),
            ..update(None, None)
        }
        .apply(&before)
        .unwrap();

        assert_eq!(after.linkedin_username, None);
        assert_eq!(after.instagram_username.as_deref(), Some("insta"));
        assert_eq!(after.career.as_deref(), Some("Engineer"));
        assert_eq!(after.profile.as_deref(), Some("new notes"));
        assert_eq!(after.role, None);
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
                linkedin_username: None,
                instagram_username: None,
                career: None,
                industry: None,
                school: None,
                grad_date: None,
                profile: None,
                role: None,
                preferred_communications: None,
                secondary_emails: None,
            }) if email == "a@example.org" && first_name == "First" && last_name == "Last"
        ));

        let with_career = CreateMember {
            career: Some("Engineer".into()),
            ..create(None)
        };
        assert!(matches!(
            &UpsertWrite::new(with_career, Some(7)),
            UpsertWrite::Update(UpdateMember {
                career: Some(Some(career)),
                secondary_emails: None,
                profile: None,
                ..
            }) if career == "Engineer"
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

    #[test]
    fn secondary_emails_keep_replace_or_clear() {
        let mut before = member(None);
        before.secondary_emails = vec!["old@example.org".into()];

        let apply = |secondary_emails| {
            UpdateMember {
                secondary_emails,
                ..update(None, None)
            }
            .apply(&before)
            .unwrap()
            .secondary_emails
        };

        assert_eq!(apply(None), ["old@example.org"]);
        assert_eq!(
            apply(Some(vec!["new@example.org".into()])),
            ["new@example.org"]
        );
        assert!(apply(Some(vec![])).is_empty());
    }

    #[test]
    fn rewriting_the_same_secondary_emails_is_unchanged() {
        let mut before = member(None);
        before.secondary_emails = vec!["old@example.org".into()];

        let after = UpdateMember {
            secondary_emails: Some(vec!["old@example.org".into()]),
            ..update(None, None)
        }
        .apply(&before)
        .unwrap();

        assert_eq!(after, before);
    }

    #[test]
    fn upsert_only_replaces_secondary_emails_when_given() {
        for given in [vec!["a2@example.org".to_string()], vec![]] {
            let with_emails = CreateMember {
                secondary_emails: Some(given.clone()),
                ..create(None)
            };
            assert!(matches!(
                UpsertWrite::new(with_emails, Some(7)),
                UpsertWrite::Update(UpdateMember {
                    secondary_emails: Some(emails),
                    ..
                }) if emails == given
            ));
        }
        assert!(matches!(
            UpsertWrite::new(create(None), Some(7)),
            UpsertWrite::Update(UpdateMember {
                secondary_emails: None,
                ..
            })
        ));
    }

    #[test]
    fn secondary_emails_never_contain_the_primary_email() {
        let stored = |emails: &[&str]| Member {
            secondary_emails: emails.iter().map(|e| e.to_string()).collect(),
            ..member(None)
        };
        let list = |emails: &[&str]| Some(emails.iter().map(|e| e.to_string()).collect());

        for (name, before, input, rejected) in [
            (
                "new primary is already a stored secondary",
                stored(&["b@example.org"]),
                UpdateMember {
                    email: Some("b@example.org".into()),
                    ..update(None, None)
                },
                true,
            ),
            (
                "new list contains the stored primary",
                stored(&[]),
                UpdateMember {
                    secondary_emails: list(&["a@example.org"]),
                    ..update(None, None)
                },
                true,
            ),
            (
                "promote: email leaves the list in the same request",
                stored(&["b@example.org"]),
                UpdateMember {
                    email: Some("b@example.org".into()),
                    secondary_emails: list(&["c@example.org"]),
                    ..update(None, None)
                },
                false,
            ),
            (
                "untouched email and list are not rechecked",
                stored(&["a@example.org"]),
                UpdateMember {
                    first_name: Some("Other".into()),
                    ..update(None, None)
                },
                false,
            ),
        ] {
            assert_eq!(input.apply(&before).is_err(), rejected, "{name}");
        }
    }
}
