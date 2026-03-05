mod common;

use ghost::db;

#[tokio::test]
async fn create_and_get_coding_session() {
    let (pool, _config, _workspace, _config_dir) = common::test_database().await;

    let session_id = db::sessions::create_session(&pool).await.unwrap();
    db::coding_sessions::create_coding_session(
        &pool,
        "cs1",
        &session_id,
        Some("chan1"),
        "/tmp/repo",
    )
    .await
    .unwrap();

    let takeover = db::coding_sessions::get_active_takeover(&pool, "chan1")
        .await
        .unwrap();
    assert!(takeover.is_some());
    let (id, sid, dir) = takeover.unwrap();
    assert_eq!(id, "cs1");
    assert_eq!(sid, session_id);
    assert_eq!(dir, "/tmp/repo");
}

#[tokio::test]
async fn end_session_clears_takeover() {
    let (pool, _config, _workspace, _config_dir) = common::test_database().await;

    let session_id = db::sessions::create_session(&pool).await.unwrap();
    db::coding_sessions::create_coding_session(
        &pool,
        "cs2",
        &session_id,
        Some("chan2"),
        "/tmp/repo",
    )
    .await
    .unwrap();

    db::coding_sessions::end_coding_session(&pool, "cs2")
        .await
        .unwrap();

    let takeover = db::coding_sessions::get_active_takeover(&pool, "chan2")
        .await
        .unwrap();
    assert!(takeover.is_none());
}

#[tokio::test]
async fn list_recent_coding_sessions() {
    let (pool, _config, _workspace, _config_dir) = common::test_database().await;

    let s1 = db::sessions::create_session(&pool).await.unwrap();
    db::coding_sessions::create_coding_session(&pool, "cs-a", &s1, None, "/a")
        .await
        .unwrap();

    let s2 = db::sessions::create_session(&pool).await.unwrap();
    db::coding_sessions::create_coding_session(&pool, "cs-b", &s2, Some("ch"), "/b")
        .await
        .unwrap();

    let list = db::coding_sessions::list_recent_coding_sessions(&pool, 10)
        .await
        .unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn reactivate_coding_session() {
    let (pool, _config, _workspace, _config_dir) = common::test_database().await;

    let session_id = db::sessions::create_session(&pool).await.unwrap();
    db::coding_sessions::create_coding_session(
        &pool,
        "cs3",
        &session_id,
        Some("chan3"),
        "/tmp/repo",
    )
    .await
    .unwrap();

    db::coding_sessions::end_coding_session(&pool, "cs3")
        .await
        .unwrap();

    // Verify ended
    let (_, _, status) = db::coding_sessions::get_coding_session(&pool, "cs3")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, "ended");

    // Reactivate
    db::coding_sessions::reactivate_coding_session(&pool, "cs3", Some("chan4"))
        .await
        .unwrap();

    let (_, _, status) = db::coding_sessions::get_coding_session(&pool, "cs3")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status, "active");

    // Takeover should be on new channel
    let takeover = db::coding_sessions::get_active_takeover(&pool, "chan4")
        .await
        .unwrap();
    assert!(takeover.is_some());
}
