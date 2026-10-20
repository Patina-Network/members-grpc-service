use prost::Message;
use sqlx::{Row, postgres::PgPoolOptions};
use tonic::Code;

use crate::proto::members::admin::{
    MemberArchived, MemberCreated, MemberUpdated, UpsertMemberRequest,
};

use super::{members_admin_service_server::MembersAdminService as _, *};

async fn disconnected_service() -> MembersAdminService {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .unwrap();
    pool.close().await;
    MembersAdminService::new(MembersAdminRepository::new(
        pool,
        crate::grpc::members_admin::encode_event,
    ))
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
async fn write_identifiers_follow_lookup_validation() {
    let service = disconnected_service().await;
    let discord_error =
        "discord_id must be a positive unsigned 64-bit integer containing only digits";
    for (email, discord_id, message) in [
        ("invalid", None, "email must be a valid email address"),
        ("member@", None, "email must be a valid email address"),
        ("a@example.org", Some(""), discord_error),
        ("a@example.org", Some("abc"), discord_error),
        ("a@example.org", Some("-1"), discord_error),
        ("a@example.org", Some("+1"), discord_error),
        ("a@example.org", Some("1.5"), discord_error),
        ("a@example.org", Some("0"), discord_error),
        ("a@example.org", Some("18446744073709551616"), discord_error),
    ] {
        let create = CreateMemberRequest {
            discord_id: discord_id.map(str::to_owned),
            ..input(email)
        };
        let update = UpdateMemberRequest {
            member_id: 1,
            email: Some(email.into()),
            discord_id: create.discord_id.clone(),
            ..Default::default()
        };
        for error in [
            service
                .create_member(admin(create.clone()))
                .await
                .unwrap_err(),
            service
                .update_member(admin(update.clone()))
                .await
                .unwrap_err(),
        ] {
            assert_eq!(error.code(), Code::InvalidArgument);
            assert_eq!(error.message(), message);
        }
        let results = service
            .bulk_member_operations(admin(BulkMemberOperationsRequest {
                operations: vec![
                    operation(Operation::Create(create.clone())),
                    operation(Operation::Update(update)),
                    operation(Operation::Upsert(UpsertMemberRequest {
                        member: Some(create),
                    })),
                ],
            }))
            .await
            .unwrap()
            .into_inner()
            .results;
        for result in results {
            let Some(Outcome::Failed(error)) = result.outcome else {
                panic!("expected validation failure");
            };
            assert_eq!(error.error, format!("{}: {message}", Code::InvalidArgument));
        }
    }
    for discord_id in [None, Some("1"), Some("18446744073709551615")] {
        let create = CreateMemberRequest {
            discord_id: discord_id.map(str::to_owned),
            ..input("member+tag@example.org")
        };
        assert_eq!(
            service
                .create_member(admin(create.clone()))
                .await
                .unwrap_err()
                .code(),
            Code::Unavailable
        );
        assert_eq!(
            service
                .update_member(admin(UpdateMemberRequest {
                    member_id: 1,
                    email: Some(create.email),
                    discord_id: create.discord_id,
                    ..Default::default()
                }))
                .await
                .unwrap_err()
                .code(),
            Code::Unavailable
        );
    }
}

#[test]
fn rejects_invalid_update_fields_and_ids() {
    for field in ["email", "first_name", "last_name"] {
        let mut input = UpdateMemberRequest {
            member_id: 1,
            ..Default::default()
        };
        match field {
            "email" => input.email = Some("  ".into()),
            "first_name" => input.first_name = Some("  ".into()),
            _ => input.last_name = Some("  ".into()),
        }
        let error = validate_update(&input).unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument);
        assert_eq!(error.message(), format!("{field} is required"));
    }
    for id in [0, -1] {
        assert_eq!(member_id(id).unwrap_err().code(), Code::InvalidArgument);
    }
}

#[tokio::test]
async fn writes_preserve_events_and_roll_back_together() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql(
        r#"
        CREATE TEMP TABLE "Member" (LIKE public."Member" INCLUDING ALL);
        CREATE TEMP TABLE "MemberEvents" (LIKE public."MemberEvents" INCLUDING ALL);
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let service = MembersAdminService::new(MembersAdminRepository::new(pool.clone(), encode_event));
    let created = service
        .create_member(admin(input("one@example.org")))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert!(created.member_id > 0);
    assert!(created.joined_at.is_some());
    assert_eq!(
        service
            .create_member(admin(input("one@example.org")))
            .await
            .unwrap_err()
            .code(),
        Code::AlreadyExists
    );

    let updated = service
        .update_member(admin(UpdateMemberRequest {
            member_id: created.member_id,
            first_name: Some("New".into()),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner()
        .member
        .unwrap();
    assert_eq!(updated.first_name, "New");
    assert_eq!(updated.email, created.email);
    assert_eq!(updated.joined_at, created.joined_at);
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
    assert_eq!(
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: created.member_id
            }))
            .await
            .unwrap()
            .into_inner()
            .member,
        Some(archived.clone())
    );
    for error in [
        service
            .update_member(admin(UpdateMemberRequest {
                member_id: i32::MAX,
                ..Default::default()
            }))
            .await
            .unwrap_err(),
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: i32::MAX,
            }))
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error.code(), Code::NotFound);
    }

    let events = sqlx::query(r#"SELECT "eventType", payload FROM "MemberEvents" ORDER BY id"#)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(
        MemberCreated::decode(events[0].get::<Vec<u8>, _>("payload").as_slice())
            .unwrap()
            .member,
        Some(created.clone())
    );
    let event = MemberUpdated::decode(events[1].get::<Vec<u8>, _>("payload").as_slice()).unwrap();
    assert_eq!(event.before, Some(created));
    assert_eq!(event.after, Some(updated));
    assert_eq!(
        event.changed_fields.unwrap().paths,
        ["first_name", "updated_at"]
    );
    assert_eq!(
        MemberArchived::decode(events[2].get::<Vec<u8>, _>("payload").as_slice())
            .unwrap()
            .member,
        Some(archived.clone())
    );

    let member = input("bulk@example.org");
    let changed = CreateMemberRequest {
        first_name: "Changed".into(),
        ..member.clone()
    };
    let results = service
        .bulk_member_operations(admin(BulkMemberOperationsRequest {
            operations: vec![
                operation(Operation::Create(member.clone())),
                operation(Operation::Create(member)),
                operation(Operation::Upsert(UpsertMemberRequest {
                    member: Some(changed.clone()),
                })),
                operation(Operation::Upsert(UpsertMemberRequest {
                    member: Some(changed),
                })),
                operation(Operation::Upsert(UpsertMemberRequest {
                    member: Some(input("new@example.org")),
                })),
            ],
        }))
        .await
        .unwrap()
        .into_inner()
        .results;
    for (index, result) in results.iter().enumerate() {
        assert_eq!(result.index, index as u32);
    }
    assert!(matches!(results[0].outcome, Some(Outcome::Created(_))));
    assert!(matches!(results[1].outcome, Some(Outcome::Failed(_))));
    let Some(Outcome::Updated(event)) = &results[2].outcome else {
        panic!("expected upsert update");
    };
    assert_eq!(event.after.as_ref().unwrap().first_name, "Changed");
    let Some(Outcome::Unchanged(unchanged)) = &results[3].outcome else {
        panic!("expected unchanged upsert");
    };
    assert_eq!(unchanged.member, event.after);
    assert!(matches!(results[4].outcome, Some(Outcome::Created(_))));

    sqlx::query(
        r#"ALTER TABLE "MemberEvents" ADD CONSTRAINT reject_events CHECK (false) NOT VALID"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    for error in [
        service
            .create_member(admin(input("rollback@example.org")))
            .await
            .unwrap_err(),
        service
            .update_member(admin(UpdateMemberRequest {
                member_id: archived.member_id,
                first_name: Some("Rollback".into()),
                ..Default::default()
            }))
            .await
            .unwrap_err(),
        service
            .archive_member(admin(ArchiveMemberRequest {
                member_id: event.after.as_ref().unwrap().member_id,
            }))
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error.code(), Code::Internal);
    }
    let stored: crate::proto::members::Member =
        crate::db::members::MembersRepository::new(pool.clone())
            .get(crate::db::members::MemberIdentifier::Id(archived.member_id))
            .await
            .unwrap()
            .unwrap()
            .into();
    assert_eq!(stored, archived);
    let count: i64 = sqlx::query_scalar(r#"SELECT count(*) FROM "Member""#)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
    pool.close().await;
}
