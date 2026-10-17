use crate::{
    config::Config,
    proto::members::admin::{MemberArchived, MemberCreated, MemberUpdated},
};
use prost::Message;
use sqlx::{
    Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use super::*;
use members_admin_service_server::MembersAdminService as _;
use tonic::Code;

fn admin<T>(input: T) -> Request<T> {
    let mut request = Request::new(input);
    request
        .metadata_mut()
        .insert("profile", "core_admin".parse().unwrap());
    request
}

fn input(email: &str) -> CreateMemberRequest {
    CreateMemberRequest {
        email: email.into(),
        first_name: "First".into(),
        last_name: "Last".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn authorization_and_validation_precede_database_access() {
    let service = MembersAdminService::new(MembersAdminRepository::new(
        PgPoolOptions::new()
            .connect_lazy("postgres://localhost/unused")
            .unwrap(),
    ));
    for profile in [
        None,
        Some(""),
        Some("admin"),
        Some("CORE_ADMIN"),
        Some("core_admin,other"),
    ] {
        let mut request = Request::new(input("a@example.org"));
        if let Some(value) = profile {
            request
                .metadata_mut()
                .insert("profile", value.parse().unwrap());
        }
        assert_eq!(
            service.create_member(request).await.unwrap_err().code(),
            Code::PermissionDenied
        );
    }
    assert_eq!(
        service
            .update_member(Request::new(UpdateMemberRequest::default()))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    assert_eq!(
        service
            .archive_member(Request::new(ArchiveMemberRequest::default()))
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    let mut duplicate = admin(input("a@example.org"));
    duplicate
        .metadata_mut()
        .append("profile", "core_admin".parse().unwrap());
    assert_eq!(
        service.create_member(duplicate).await.unwrap_err().code(),
        Code::PermissionDenied
    );
    for field in 0..3 {
        let mut value = input("a@example.org");
        match field {
            0 => value.email.clear(),
            1 => value.first_name.clear(),
            _ => value.last_name = "  ".into(),
        }
        assert_eq!(
            service
                .create_member(admin(value))
                .await
                .unwrap_err()
                .code(),
            Code::InvalidArgument
        );
    }
    assert_eq!(
        service
            .update_member(admin(UpdateMemberRequest::default()))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    assert_eq!(
        service
            .archive_member(admin(ArchiveMemberRequest::default()))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let mut value = input("a@example.org");
    value.joined_at = Some(prost_types::Timestamp {
        seconds: 0,
        nanos: -1,
    });
    assert_eq!(
        service
            .create_member(admin(value))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
}

#[tokio::test]
#[ignore = "requires a disposable Postgres database and DATABASE_* environment variables"]
async fn stored_writes_and_atomic_events() {
    let config = Config::from_env().unwrap();
    let options = PgConnectOptions::new()
        .host(&config.database_host)
        .port(config.database_port)
        .database(&config.database_name)
        .username(&config.database_user)
        .password(&config.database_password);
    let setup_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let schema: String =
        sqlx::query_scalar("SELECT 'member_test_' || replace(gen_random_uuid()::text, '-', '')")
            .fetch_one(&setup_pool)
            .await
            .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&setup_pool)
        .await
        .unwrap();
    let test_schema = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _| {
            let schema = test_schema.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('search_path', $1, false)")
                    .bind(schema)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../db/migration/0001_create_members_table.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../db/migration/0002_create_member_events.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let service = MembersAdminService::new(MembersAdminRepository::new(pool.clone()));
    let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
        .await
        .unwrap();
    listener.listen("MemberCreated").await.unwrap();
    listener.listen("MemberUpdated").await.unwrap();
    listener.listen("MemberArchived").await.unwrap();
    let created = service
        .create_member(admin(input("one@example.org")))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert!(!created.member_id.is_empty());
    assert!(created.joined_at.is_some());
    assert!(created.updated_at.is_some());
    assert!(created.archived_at.is_none());
    assert_eq!(
        service
            .create_member(admin(input("one@example.org")))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );
    let joined = prost_types::Timestamp {
        seconds: 253_402_300_799,
        nanos: 123_456_000,
    };
    let mut second = input("two@example.org");
    second.discord_id = Some("discord-2".into());
    second.discord_username = Some("two".into());
    second.joined_at = Some(joined);
    let second = service
        .create_member(admin(second))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert_eq!(second.joined_at, Some(joined));
    let mut duplicate = input("three@example.org");
    duplicate.discord_id = second.discord_id.clone();
    assert_eq!(
        service
            .create_member(admin(duplicate))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );
    for patch in [
        UpdateMemberRequest {
            email: Some(second.email.clone()),
            ..Default::default()
        },
        UpdateMemberRequest {
            discord_id: second.discord_id.clone(),
            ..Default::default()
        },
    ] {
        assert_eq!(
            service
                .update_member(admin(UpdateMemberRequest {
                    member_id: created.member_id.clone(),
                    ..patch
                }))
                .await
                .unwrap_err()
                .code(),
            Code::AlreadyExists
        );
    }
    let updated = service
        .update_member(admin(UpdateMemberRequest {
            member_id: created.member_id.clone(),
            first_name: Some("New".into()),
            discord_username: Some(String::new()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert_eq!(updated.first_name, "New");
    assert_eq!(updated.email, created.email);
    assert_eq!(updated.last_name, created.last_name);
    assert_eq!(updated.joined_at, created.joined_at);
    assert_eq!(updated.discord_username.as_deref(), Some(""));
    let updated_time = updated.updated_at.as_ref().unwrap();
    let created_time = created.updated_at.as_ref().unwrap();
    assert!(
        (updated_time.seconds, updated_time.nanos) > (created_time.seconds, created_time.nanos)
    );
    let archived = service
        .archive_member(admin(ArchiveMemberRequest {
            member_id: created.member_id.clone(),
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.archived_at, archived.updated_at);
    assert_eq!(archived.first_name, updated.first_name);
    let count: i64 = sqlx::query_scalar(r#"SELECT count(*) FROM "Member""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    let absent = "00000000-0000-0000-0000-000000000000".to_owned();
    assert_eq!(
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: absent.clone()
            }))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        service
            .update_member(admin(UpdateMemberRequest {
                member_id: absent,
                ..Default::default()
            }))
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );
    assert_eq!(
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: "invalid".into()
            }))
            .await
            .unwrap_err()
            .code(),
        Code::InvalidArgument
    );
    let events = sqlx::query("SELECT id, event_type, payload FROM member_events ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events.len(), 4);
    for event in &events {
        let notification = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(notification.channel(), event.get::<String, _>("event_type"));
        assert_eq!(
            notification.payload(),
            event.get::<i64, _>("id").to_string()
        );
    }
    assert_eq!(
        MemberCreated::decode(events[0].get::<Vec<u8>, _>("payload").as_slice())
            .unwrap()
            .member,
        Some(created.clone())
    );
    let event = MemberUpdated::decode(events[2].get::<Vec<u8>, _>("payload").as_slice()).unwrap();
    assert_eq!(event.before, Some(created));
    assert_eq!(event.after, Some(updated));
    assert_eq!(
        event.changed_fields.unwrap().paths,
        ["first_name", "discord_username", "updated_at"]
    );
    assert_eq!(events[3].get::<String, _>("event_type"), "MemberArchived");
    assert_eq!(
        MemberArchived::decode(events[3].get::<Vec<u8>, _>("payload").as_slice())
            .unwrap()
            .member,
        Some(archived.clone())
    );
    let joined = prost_types::Timestamp {
        seconds: -1,
        nanos: 987_654_000,
    };
    let edited = service
        .update_member(admin(UpdateMemberRequest {
            member_id: second.member_id.clone(),
            email: Some("changed@example.org".into()),
            first_name: Some("Changed".into()),
            last_name: Some("Name".into()),
            joined_at: Some(joined),
            discord_id: second.discord_id.clone(),
            discord_username: Some("changed".into()),
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert_eq!(edited.member_id, second.member_id);
    assert_eq!(edited.email, "changed@example.org");
    assert_eq!(edited.first_name, "Changed");
    assert_eq!(edited.last_name, "Name");
    assert_eq!(edited.joined_at, Some(joined));
    assert_eq!(edited.discord_id, second.discord_id);
    assert_eq!(edited.discord_username.as_deref(), Some("changed"));
    assert!(edited.archived_at.is_none());

    let archived = MemberArchived::decode(events[3].get::<Vec<u8>, _>("payload").as_slice())
        .unwrap()
        .member
        .unwrap();
    let mut unchanged = service
        .update_member(admin(UpdateMemberRequest {
            member_id: archived.member_id.clone(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    let previous = archived.updated_at.as_ref().unwrap();
    let current = unchanged.updated_at.as_ref().unwrap();
    assert!((current.seconds, current.nanos) > (previous.seconds, previous.nanos));
    unchanged.updated_at = archived.updated_at;
    assert_eq!(unchanged, archived);

    let snapshot_sql = r#"SELECT row_to_json(m)::text FROM "Member" m WHERE id = $1::uuid"#;
    let before_failure: String = sqlx::query_scalar(snapshot_sql)
        .bind(&edited.member_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    sqlx::query("ALTER TABLE member_events ADD CONSTRAINT reject_events CHECK (false) NOT VALID")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        service
            .create_member(admin(input("rollback@example.org")))
            .await
            .unwrap_err()
            .code(),
        Code::Internal
    );
    for result in [
        service
            .update_member(admin(UpdateMemberRequest {
                member_id: edited.member_id.clone(),
                first_name: Some("Must roll back".into()),
                ..Default::default()
            }))
            .await
            .map(|_| ()),
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: edited.member_id.clone(),
            }))
            .await
            .map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err().code(), Code::Internal);
    }
    let after_failure: String = sqlx::query_scalar(snapshot_sql)
        .bind(&edited.member_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after_failure, before_failure);
    let count: i64 = sqlx::query_scalar(r#"SELECT count(*) FROM "Member""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    drop(listener);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&setup_pool)
        .await
        .unwrap();
    setup_pool.close().await;
}
