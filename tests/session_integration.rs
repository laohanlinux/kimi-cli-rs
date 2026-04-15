use std::path::PathBuf;

fn temp_share_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kimi-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    kimi_cli_rs::share::set_test_share_dir(dir.clone());
    dir
}

#[tokio::test]
async fn session_create_find_list_delete_roundtrip() {
    let _share = temp_share_dir();
    let work_dir = std::env::temp_dir().join(format!("kimi-wd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work_dir).unwrap();

    let session = kimi_cli_rs::session::create(work_dir.clone(), None, None)
        .await
        .expect("create session");

    assert!(session.dir().exists());
    assert_eq!(
        dunce::canonicalize(&session.work_dir).unwrap(),
        dunce::canonicalize(&work_dir).unwrap()
    );

    let found = kimi_cli_rs::session::find(session.work_dir.clone(), &session.id)
        .await
        .expect("find session");
    assert_eq!(found.id, session.id);

    // Make session non-empty so list includes it
    std::fs::write(
        &session.context_file,
        r#"{"role":"user","content":"hello"}"#,
    )
    .unwrap();

    let list = kimi_cli_rs::session::list(session.work_dir.clone()).await;
    assert_eq!(list.len(), 1);

    let session_dir = session.dir();
    session.delete().await.expect("delete session");
    assert!(!session_dir.exists());
}

#[tokio::test]
async fn session_continue_returns_latest() {
    let _share = temp_share_dir();
    let work_dir = std::env::temp_dir().join(format!("kimi-wd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work_dir).unwrap();

    let _s1 = kimi_cli_rs::session::create(work_dir.clone(), None, None)
        .await
        .unwrap();
    let s2 = kimi_cli_rs::session::create(work_dir.clone(), None, None)
        .await
        .unwrap();

    let continued = kimi_cli_rs::session::continue_(work_dir.clone())
        .await
        .expect("continue should return latest");
    assert_eq!(continued.id, s2.id);
}
