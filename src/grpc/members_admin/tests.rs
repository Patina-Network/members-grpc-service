use crate::proto::members::admin::{
    MemberArchived, MemberCreated, MemberUpdated, UpsertMemberRequest,
};
use prost::Message;
use sqlx::{Row, postgres::PgPoolOptions};

use super::{members_admin_service_server::MembersAdminService as _, *};
use tonic::Code;

async fn disconnected_service() -> MembersAdminService {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    pool.close().await;
    MembersAdminService::new(MembersAdminRepository::new(pool))
}

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
    let service = disconnected_service().await;
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
async fn update_rejects_blank_email_and_names_before_database_access() {
    let service = disconnected_service().await;
    for field in ["email", "first_name", "last_name"] {
        for blank in ["", " \t\n "] {
            let mut input = UpdateMemberRequest {
                member_id: 1,
                ..Default::default()
            };
            match field {
                "email" => input.email = Some(blank.into()),
                "first_name" => input.first_name = Some(blank.into()),
                _ => input.last_name = Some(blank.into()),
            }
            let error = service.update_member(admin(input)).await.unwrap_err();
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), format!("{field} is required"));
        }
    }
}

#[tokio::test]
async fn writes_reject_nonpositive_ids_before_database_access() {
    let service = disconnected_service().await;
    for member_id in [0, -1, i32::MIN] {
        let update = service
            .update_member(admin(UpdateMemberRequest {
                member_id,
                ..Default::default()
            }))
            .await
            .unwrap_err();
        let archive = service
            .archive_member(admin(ArchiveMemberRequest { member_id }))
            .await
            .unwrap_err();
        for error in [update, archive] {
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), "member_id must be a positive integer");
        }
    }
}

fn operation(operation: Operation) -> MemberOperation {
    MemberOperation {
        operation: Some(operation),
    }
}

#[tokio::test]
async fn bulk_authorization_validation_and_failures() {
    let service = disconnected_service().await;

    let error = service
        .bulk_member_operations(Request::new(BulkMemberOperationsRequest::default()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::PermissionDenied);
    assert!(
        service
            .bulk_member_operations(admin(BulkMemberOperationsRequest::default()))
            .await
            .unwrap()
            .into_inner()
            .results
            .is_empty()
    );

    let operations = vec![
        MemberOperation::default(),
        operation(Operation::Upsert(UpsertMemberRequest::default())),
        operation(Operation::Create(input(""))),
        operation(Operation::Update(UpdateMemberRequest {
            member_id: 1,
            first_name: Some("  ".into()),
            ..Default::default()
        })),
        operation(Operation::Archive(ArchiveMemberRequest { member_id: 0 })),
        operation(Operation::Upsert(UpsertMemberRequest {
            member: Some(CreateMemberRequest {
                joined_at: Some(prost_types::Timestamp {
                    seconds: 0,
                    nanos: -1,
                }),
                ..input("valid@example.org")
            }),
        })),
        operation(Operation::Create(input("valid@example.org"))),
    ];
    let results = service
        .bulk_member_operations(admin(BulkMemberOperationsRequest { operations }))
        .await
        .unwrap()
        .into_inner()
        .results;
    assert_eq!(results.len(), 7);
    for (index, result) in results.into_iter().enumerate() {
        assert_eq!(result.index, index as u32);
        let Some(Outcome::Failed(failure)) = result.outcome else {
            panic!("expected per-operation failure")
        };
        let expected = [
            "operation is required",
            "upsert member is required",
            "email is required",
            "first_name is required",
            "member_id must be a positive integer",
            "joined_at is invalid",
            "database unavailable",
        ][index];
        assert!(failure.error.contains(expected), "{}", failure.error);
    }
}

#[tokio::test]
async fn stored_writes_and_atomic_events() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let setup_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let schema: String =
        sqlx::query_scalar("SELECT 'member_test_' || replace(gen_random_uuid()::text, '-', '')")
            .fetch_one(&setup_pool)
            .await
            .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
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
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../db/all/0001_create_members_table.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../db/all/0002_integer_member_ids.up.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../db/all/0003_create_member_events.up.sql"
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
    assert!(created.member_id > 0);
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
                    member_id: created.member_id,
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
            member_id: created.member_id,
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
            member_id: created.member_id,
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.archived_at, archived.updated_at);
    assert_eq!(archived.first_name, updated.first_name);
    let retried = service
        .archive_member(admin(ArchiveMemberRequest {
            member_id: archived.member_id,
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert_eq!(retried, archived);
    let reads = crate::db::members::MembersRepository::new(pool.clone());
    let stored: crate::proto::members::Member = reads
        .get(crate::db::members::MemberIdentifier::Id(archived.member_id))
        .await
        .unwrap()
        .unwrap()
        .into();
    assert_eq!(stored, archived);
    let absent = i32::MAX;
    assert_eq!(
        service
            .archive_member(admin(ArchiveMemberRequest { member_id: absent }))
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
    let events = sqlx::query(r#"SELECT id, "eventType", payload FROM "MemberEvents" ORDER BY id"#)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events.len(), 4);
    for event in &events {
        let notification = tokio::time::timeout(std::time::Duration::from_secs(5), listener.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(notification.channel(), event.get::<String, _>("eventType"));
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
    assert_eq!(events[3].get::<String, _>("eventType"), "MemberArchived");
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
            member_id: second.member_id,
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

    let mut unchanged = service
        .update_member(admin(UpdateMemberRequest {
            member_id: archived.member_id,
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

    let snapshot_sql = r#"SELECT row_to_json(m)::text FROM "Member" m WHERE id = $1"#;
    let before_failure: String = sqlx::query_scalar(snapshot_sql)
        .bind(edited.member_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    sqlx::query(
        r#"ALTER TABLE "MemberEvents" ADD CONSTRAINT reject_events CHECK (false) NOT VALID"#,
    )
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
                member_id: edited.member_id,
                first_name: Some("Must roll back".into()),
                ..Default::default()
            }))
            .await
            .map(|_| ()),
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: edited.member_id,
            }))
            .await
            .map(|_| ()),
    ] {
        assert_eq!(result.unwrap_err().code(), Code::Internal);
    }
    let after_failure: String = sqlx::query_scalar(snapshot_sql)
        .bind(edited.member_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after_failure, before_failure);
    let count: i64 = sqlx::query_scalar(r#"SELECT count(*) FROM "Member""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    bulk_writes_and_events(&service, &pool).await;
    drop(listener);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&setup_pool)
        .await
        .unwrap();
    setup_pool.close().await;
}

async fn bulk_writes_and_events(service: &MembersAdminService, pool: &sqlx::PgPool) {
    let failed = service
        .bulk_member_operations(admin(BulkMemberOperationsRequest {
            operations: vec![operation(Operation::Upsert(UpsertMemberRequest {
                member: Some(input("bulk-rollback@example.org")),
            }))],
        }))
        .await
        .unwrap()
        .into_inner()
        .results;
    assert!(matches!(failed[0].outcome, Some(Outcome::Failed(_))));
    let count: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM "Member" WHERE email = 'bulk-rollback@example.org'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    sqlx::query(r#"ALTER TABLE "MemberEvents" DROP CONSTRAINT reject_events"#)
        .execute(pool)
        .await
        .unwrap();

    let initial_events: i64 = sqlx::query_scalar(r#"SELECT count(*) FROM "MemberEvents""#)
        .fetch_one(pool)
        .await
        .unwrap();
    let member = input("bulk@example.org");
    let results = service
        .bulk_member_operations(admin(BulkMemberOperationsRequest {
            operations: vec![
                operation(Operation::Create(member.clone())),
                operation(Operation::Create(member.clone())),
                operation(Operation::Upsert(UpsertMemberRequest {
                    member: Some(CreateMemberRequest {
                        first_name: "Changed".into(),
                        ..member.clone()
                    }),
                })),
                operation(Operation::Upsert(UpsertMemberRequest {
                    member: Some(CreateMemberRequest {
                        first_name: "Changed".into(),
                        ..member
                    }),
                })),
                operation(Operation::Upsert(UpsertMemberRequest {
                    member: Some(input("bulk-new@example.org")),
                })),
            ],
        }))
        .await
        .unwrap()
        .into_inner()
        .results;
    assert_eq!(results.len(), 5);
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result.index, index as u32);
    }
    let Some(Outcome::Created(created)) = &results[0].outcome else {
        panic!("expected create")
    };
    let created = created.member.as_ref().unwrap();
    assert!(matches!(results[1].outcome, Some(Outcome::Failed(_))));
    let Some(Outcome::Updated(updated)) = &results[2].outcome else {
        panic!("expected upsert update")
    };
    assert_eq!(updated.before.as_ref(), Some(created));
    assert_eq!(updated.after.as_ref().unwrap().first_name, "Changed");
    assert!(
        updated
            .changed_fields
            .as_ref()
            .unwrap()
            .paths
            .contains(&"first_name".into())
    );
    let Some(Outcome::Unchanged(unchanged)) = &results[3].outcome else {
        panic!("expected unchanged upsert")
    };
    assert_eq!(unchanged.member, updated.after);
    assert!(matches!(results[4].outcome, Some(Outcome::Created(_))));

    let id = created.member_id;
    let results = service
        .bulk_member_operations(admin(BulkMemberOperationsRequest {
            operations: vec![
                operation(Operation::Update(UpdateMemberRequest {
                    member_id: id,
                    last_name: Some("Bulk".into()),
                    ..Default::default()
                })),
                operation(Operation::Update(UpdateMemberRequest {
                    member_id: id,
                    ..Default::default()
                })),
                operation(Operation::Archive(ArchiveMemberRequest {
                    member_id: i32::MAX,
                })),
                operation(Operation::Archive(ArchiveMemberRequest { member_id: id })),
                operation(Operation::Archive(ArchiveMemberRequest { member_id: id })),
            ],
        }))
        .await
        .unwrap()
        .into_inner()
        .results;
    let Some(Outcome::Updated(updated)) = &results[0].outcome else {
        panic!("expected update")
    };
    assert_eq!(updated.after.as_ref().unwrap().last_name, "Bulk");
    let Some(Outcome::Unchanged(unchanged)) = &results[1].outcome else {
        panic!("expected unchanged update")
    };
    assert_eq!(unchanged.member, updated.after);
    assert!(matches!(results[2].outcome, Some(Outcome::Failed(_))));
    let Some(Outcome::Archived(archived)) = &results[3].outcome else {
        panic!("expected archive")
    };
    let Some(Outcome::Unchanged(retried)) = &results[4].outcome else {
        panic!("expected unchanged archive retry")
    };
    assert_eq!(retried.member, archived.member);
    let events =
        sqlx::query(r#"SELECT "eventType", payload FROM "MemberEvents" ORDER BY id OFFSET $1"#)
            .bind(initial_events)
            .fetch_all(pool)
            .await
            .unwrap();
    assert_eq!(events.len(), 5);
    assert_eq!(
        events
            .iter()
            .map(|row| row.get::<String, _>("eventType"))
            .collect::<Vec<_>>(),
        [
            "MemberCreated",
            "MemberUpdated",
            "MemberCreated",
            "MemberUpdated",
            "MemberArchived"
        ]
    );
    let event = MemberUpdated::decode(events[3].get::<Vec<u8>, _>("payload").as_slice()).unwrap();
    assert_eq!(&event, updated.as_ref());
    let event = MemberArchived::decode(events[4].get::<Vec<u8>, _>("payload").as_slice()).unwrap();
    assert_eq!(&event, archived);
}
