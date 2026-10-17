use members_grpc_service::{
    config::Config,
    db::members::MembersRepository,
    grpc::members::{MembersService, MembersServiceServer},
    proto::members::{
        GetMemberRequest, ListMembersRequest, get_member_request::Identifier,
        members_service_client::MembersServiceClient,
    },
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Code, Request};
use uuid::Uuid;

struct ServerTask(tokio::task::JoinHandle<Result<(), tonic::transport::Error>>);

impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn id(value: u64) -> String {
    format!("00000000-0000-0000-0000-{value:012}")
}

#[tokio::test]
#[ignore = "requires a migrated Postgres database and DATABASE_* environment variables"]
async fn postgres_member_reads() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(
            PgConnectOptions::new()
                .host(&config.database_host)
                .port(config.database_port)
                .database(&config.database_name)
                .username(&config.database_user)
                .password(&config.database_password),
        )
        .await?;
    sqlx::query(r#"CREATE TEMP TABLE "Member" (LIKE public."Member" INCLUDING ALL)"#)
        .execute(&pool)
        .await?;
    let service = MembersService::new(MembersRepository::new(pool.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = ServerTask(tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(MembersServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
    }));
    let mut service = MembersServiceClient::connect(format!("http://{address}")).await?;

    let empty = service
        .list_members(Request::new(ListMembersRequest::default()))
        .await?
        .into_inner();
    assert!(empty.members.is_empty());
    assert!(empty.next_page_token.is_empty());

    sqlx::query(
        r#"INSERT INTO "Member" (
            id, email, "firstName", "lastName", "joinedAt",
            "discordId", "discordUsername", "updatedAt", "archivedAt"
        ) SELECT
            ('00000000-0000-0000-0000-' || lpad(i::text, 12, '0'))::uuid,
            'member' || i || '@example.com', 'First', 'Last',
            '2026-01-02T03:04:05.123456Z'::timestamptz,
            CASE WHEN i = 1 THEN 'discord-1' END,
            CASE WHEN i = 1 THEN 'discord-user' END,
            '2026-02-03T04:05:06.654321Z'::timestamptz,
            CASE WHEN i = 3 THEN '2026-03-04T05:06:07Z'::timestamptz END
        FROM generate_series(1, 4) AS i"#,
    )
    .execute(&pool)
    .await?;

    for identifier in [
        Identifier::MemberId(id(1)),
        Identifier::Email("member1@example.com".into()),
        Identifier::DiscordId("discord-1".into()),
    ] {
        let member = service
            .get_member(Request::new(GetMemberRequest {
                identifier: Some(identifier),
            }))
            .await?
            .into_inner()
            .member
            .unwrap();
        assert_eq!(member.member_id, id(1));
        assert_eq!(member.email, "member1@example.com");
        assert_eq!(member.first_name, "First");
        assert_eq!(member.last_name, "Last");
        assert_eq!(member.discord_id.as_deref(), Some("discord-1"));
        assert_eq!(member.discord_username.as_deref(), Some("discord-user"));
        assert_eq!(
            member.joined_at.unwrap(),
            "2026-01-02T03:04:05.123456Z".parse()?
        );
        assert_eq!(
            member.updated_at.unwrap(),
            "2026-02-03T04:05:06.654321Z".parse()?
        );
        assert!(member.archived_at.is_none());
    }

    for identifier in [
        Identifier::MemberId(id(999)),
        Identifier::Email("missing@example.com".into()),
        Identifier::DiscordId("missing".into()),
        Identifier::Email("' OR TRUE --".into()),
        Identifier::DiscordId("' OR TRUE --".into()),
    ] {
        let error = service
            .get_member(Request::new(GetMemberRequest {
                identifier: Some(identifier),
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::NotFound);
    }

    let archived = service
        .get_member(Request::new(GetMemberRequest {
            identifier: Some(Identifier::MemberId(id(3))),
        }))
        .await?
        .into_inner()
        .member
        .unwrap();
    assert_eq!(
        archived.archived_at.unwrap(),
        "2026-03-04T05:06:07Z".parse()?
    );
    assert!(archived.discord_id.is_none());
    assert!(archived.discord_username.is_none());

    let first = service
        .list_members(Request::new(ListMembersRequest {
            page_size: 2,
            ..Default::default()
        }))
        .await?
        .into_inner();
    assert_eq!(
        first
            .members
            .iter()
            .map(|m| m.member_id.clone())
            .collect::<Vec<_>>(),
        [id(1), id(2)]
    );
    assert!(!first.next_page_token.is_empty());

    sqlx::query(r#"DELETE FROM "Member" WHERE id = $1"#)
        .bind(Uuid::parse_str(&id(1))?)
        .execute(&pool)
        .await?;
    let second = service
        .list_members(Request::new(ListMembersRequest {
            page_size: 2,
            page_token: first.next_page_token,
            ..Default::default()
        }))
        .await?
        .into_inner();
    assert_eq!(second.members.len(), 1);
    assert_eq!(second.members[0].member_id, id(4));
    assert!(second.next_page_token.is_empty());

    let with_archived = service
        .list_members(Request::new(ListMembersRequest {
            include_archived: true,
            page_size: 3,
            ..Default::default()
        }))
        .await?
        .into_inner();
    assert_eq!(
        with_archived
            .members
            .iter()
            .map(|m| m.member_id.clone())
            .collect::<Vec<_>>(),
        [id(2), id(3), id(4)]
    );
    assert!(with_archived.next_page_token.is_empty());

    sqlx::query(
        r#"INSERT INTO "Member" (id, email, "firstName", "lastName", "joinedAt", "updatedAt")
        SELECT ('00000000-0000-0000-0000-' || lpad(i::text, 12, '0'))::uuid,
               'member' || i || '@example.com', 'First', 'Last', now(), now()
        FROM generate_series(5, 105) AS i"#,
    )
    .execute(&pool)
    .await?;
    for (requested, expected) in [(0, 50), (i32::MAX, 100)] {
        let page = service
            .list_members(Request::new(ListMembersRequest {
                page_size: requested,
                ..Default::default()
            }))
            .await?
            .into_inner();
        assert_eq!(page.members.len(), expected);
        assert!(!page.next_page_token.is_empty());
    }
    pool.close().await;
    Ok(())
}
