use std::path::PathBuf;

fn temp_share_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kimi-web-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    kimi_cli_rs::share::set_test_share_dir(dir.clone());
    dir
}

async fn start_test_server() -> (u16, reqwest::Client) {
    let state = kimi_cli_rs::web::api::WebAppState::default();
    let app = kimi_cli_rs::web::api::router().with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();
    // wait a moment for the server to be ready
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (port, client)
}

#[tokio::test]
async fn healthz_returns_ok() {
    let (port, client) = start_test_server().await;
    let resp = client
        .get(&format!("http://127.0.0.1:{port}/healthz"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn session_crud_roundtrip() {
    let _share = temp_share_dir();
    let (port, client) = start_test_server().await;
    let base = format!("http://127.0.0.1:{port}/api/sessions");

    // create
    let work_dir = std::env::temp_dir().join(format!("kimi-wd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work_dir).unwrap();
    let create_resp = client
        .post(&base)
        .json(&serde_json::json!({
            "work_dir": work_dir.to_string_lossy().to_string(),
            "create_dir": false,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();

    // get
    let get_resp = client.get(&format!("{base}/{id}")).send().await.unwrap();
    assert_eq!(get_resp.status(), 200);
    let got: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(got["id"], id);

    // list
    let list_resp = client.get(&base).send().await.unwrap();
    assert_eq!(list_resp.status(), 200);
    let list: serde_json::Value = list_resp.json().await.unwrap();
    let sessions = list["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());

    // update
    let patch_resp = client
        .patch(&format!("{base}/{id}"))
        .json(&serde_json::json!({ "title": "Updated Title" }))
        .send()
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), 200);
    let patched: serde_json::Value = patch_resp.json().await.unwrap();
    assert_eq!(patched["title"], "Updated Title");

    // delete
    let del_resp = client.delete(&format!("{base}/{id}")).send().await.unwrap();
    assert_eq!(del_resp.status(), 204);

    // get after delete should 404 (from store only; session dir may still exist on disk,
    // but store removed it)
    let get_resp2 = client.get(&format!("{base}/{id}")).send().await.unwrap();
    assert_eq!(get_resp2.status(), 404);
}

#[tokio::test]
async fn git_diff_for_non_git_repo() {
    let _share = temp_share_dir();
    let (port, client) = start_test_server().await;

    let work_dir = std::env::temp_dir().join(format!("kimi-wd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work_dir).unwrap();
    let create_resp = client
        .post(&format!("http://127.0.0.1:{port}/api/sessions"))
        .json(&serde_json::json!({
            "work_dir": work_dir.to_string_lossy().to_string(),
        }))
        .send()
        .await
        .unwrap();
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let id = created["id"].as_str().unwrap();

    let diff_resp = client
        .get(&format!(
            "http://127.0.0.1:{port}/api/sessions/{id}/git-diff"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(diff_resp.status(), 200);
    let diff: serde_json::Value = diff_resp.json().await.unwrap();
    assert_eq!(diff["is_git_repo"], false);
}
