//! End-to-end repository tests against `sqlite::memory:`. Postgres support
//! comes online when CI gains a Postgres service.

use drive_db::{
    Db, DbError, FileRepo, FolderRepo, NewFile, NewFolder, NewSession, NewUser, SessionRepo,
    UserRepo,
};

async fn fresh_db() -> Db {
    Db::connect("sqlite::memory:").await.expect("connect")
}

#[tokio::test]
async fn migrate_then_users_roundtrip() {
    let db = fresh_db().await;
    let users = UserRepo::new(&db);

    let u = users
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: "$argon2id$dummy".into(),
            is_admin: true,
        })
        .await
        .expect("insert");
    assert!(u.is_admin);

    let by_username = users.find_by_username("admin").await.expect("find");
    assert_eq!(by_username.id, u.id);
    assert!(by_username.is_admin);

    let by_id = users.find_by_id(&u.id).await.expect("find by id");
    assert_eq!(by_id.username, "admin");

    let missing = users.find_by_username("nobody").await;
    assert!(matches!(missing, Err(DbError::NotFound)));
}

#[tokio::test]
async fn users_unique_username() {
    let db = fresh_db().await;
    let users = UserRepo::new(&db);
    users
        .insert(&NewUser {
            username: "dup".into(),
            password_hash: "h".into(),
            is_admin: false,
        })
        .await
        .expect("first insert");
    let err = users
        .insert(&NewUser {
            username: "dup".into(),
            password_hash: "h".into(),
            is_admin: false,
        })
        .await
        .expect_err("second must fail");
    assert!(matches!(err, DbError::UniqueViolation(_)));
}

#[tokio::test]
async fn sessions_create_get_delete() {
    let db = fresh_db().await;
    let users = UserRepo::new(&db);
    let sessions = SessionRepo::new(&db);

    let u = users
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: "h".into(),
            is_admin: true,
        })
        .await
        .unwrap();

    let s = sessions
        .insert(
            "session-id-1",
            &NewSession {
                user_id: u.id.clone(),
                csrf_token: "csrf".into(),
                ttl: time::Duration::hours(24),
            },
        )
        .await
        .unwrap();
    assert_eq!(s.user_id, u.id);
    assert!(!s.is_expired());

    let fetched = sessions.get("session-id-1").await.unwrap();
    assert_eq!(fetched.csrf_token, "csrf");

    sessions.delete("session-id-1").await.unwrap();
    assert!(matches!(
        sessions.get("session-id-1").await,
        Err(DbError::NotFound)
    ));
}

async fn seed_admin(db: &Db) -> String {
    UserRepo::new(db)
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: "h".into(),
            is_admin: true,
        })
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn folders_create_list_rename_move_trash_restore() {
    let db = fresh_db().await;
    let owner = seed_admin(&db).await;
    let repo = FolderRepo::new(&db);

    let f1 = repo
        .insert(&NewFolder {
            parent_id: None,
            name: "Reports".into(),
            owner_id: owner.clone(),
        })
        .await
        .unwrap();
    let f2 = repo
        .insert(&NewFolder {
            parent_id: Some(f1.id.clone()),
            name: "Q2".into(),
            owner_id: owner.clone(),
        })
        .await
        .unwrap();

    let root = repo.list_children(None, &owner).await.unwrap();
    assert_eq!(root.len(), 1);
    assert_eq!(root[0].id, f1.id);

    let kids = repo.list_children(Some(&f1.id), &owner).await.unwrap();
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].name, "Q2");

    repo.rename(&f2.id, "Q2-renamed").await.unwrap();
    assert_eq!(repo.find_by_id(&f2.id).await.unwrap().name, "Q2-renamed");

    repo.trash(&f2.id).await.unwrap();
    assert!(repo.find_by_id(&f2.id).await.unwrap().trashed_at.is_some());
    assert!(repo
        .list_children(Some(&f1.id), &owner)
        .await
        .unwrap()
        .is_empty());
    repo.restore(&f2.id).await.unwrap();
    let restored = repo.find_by_id(&f2.id).await.unwrap();
    assert!(restored.trashed_at.is_none());
    assert_eq!(restored.parent_id.as_deref(), Some(f1.id.as_str()));
}

#[tokio::test]
async fn files_insert_list_rename_overwrite_trash_restore() {
    let db = fresh_db().await;
    let owner = seed_admin(&db).await;
    let folders = FolderRepo::new(&db);
    let files = FileRepo::new(&db);
    let root_folder = folders
        .insert(&NewFolder {
            parent_id: None,
            name: "Home".into(),
            owner_id: owner.clone(),
        })
        .await
        .unwrap();

    let id = ulid::Ulid::new().to_string();
    files
        .insert(&NewFile {
            id: id.clone(),
            parent_id: Some(root_folder.id.clone()),
            name: "Budget Q2.xlsx".into(),
            size: 42,
            content_type: Some(
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
            ),
            etag: None,
            owner_id: owner.clone(),
            thumbnail: None,
        })
        .await
        .unwrap();

    let list = files
        .list_children(Some(&root_folder.id), &owner)
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Budget Q2.xlsx");

    files.rename(&id, "Budget Q2 — final.xlsx").await.unwrap();
    files
        .record_overwrite(&id, 100, Some("etag-1"))
        .await
        .unwrap();
    let f = files.find_by_id(&id).await.unwrap();
    assert_eq!(f.name, "Budget Q2 — final.xlsx");
    assert_eq!(f.size, 100);
    assert_eq!(f.version, 2);

    files.trash(&id).await.unwrap();
    assert!(files
        .list_children(Some(&root_folder.id), &owner)
        .await
        .unwrap()
        .is_empty());
    files.restore(&id).await.unwrap();
    assert_eq!(
        files.find_by_id(&id).await.unwrap().parent_id.as_deref(),
        Some(root_folder.id.as_str())
    );
}

#[tokio::test]
async fn sessions_janitor_clears_expired() {
    let db = fresh_db().await;
    let users = UserRepo::new(&db);
    let sessions = SessionRepo::new(&db);
    let u = users
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: "h".into(),
            is_admin: true,
        })
        .await
        .unwrap();
    sessions
        .insert(
            "live",
            &NewSession {
                user_id: u.id.clone(),
                csrf_token: "c".into(),
                ttl: time::Duration::hours(1),
            },
        )
        .await
        .unwrap();
    sessions
        .insert(
            "dead",
            &NewSession {
                user_id: u.id.clone(),
                csrf_token: "c".into(),
                ttl: time::Duration::seconds(-1),
            },
        )
        .await
        .unwrap();

    let cleaned = sessions.delete_expired().await.unwrap();
    assert_eq!(cleaned, 1);
    assert!(sessions.get("live").await.is_ok());
    assert!(matches!(sessions.get("dead").await, Err(DbError::NotFound)));
}
