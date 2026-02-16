mod common;

use ghost::db;

#[tokio::test]
async fn schema_apply_is_idempotent() {
    let (_db, config, _workspace, _config_dir) = common::test_database().await;

    let reconnect = db::connect(&config.workspace).await;
    assert!(reconnect.is_ok());
}

#[tokio::test]
async fn session_message_note_and_edges_work() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let session = db::sessions::get_session(&db, &session_id)
        .await
        .expect("get session");
    assert_eq!(session.id, session_id);

    db::sessions::update_activity(&db, &session_id)
        .await
        .expect("update activity");

    let _message_id = db::sessions::create_message(&db, &session_id, "user", "hello")
        .await
        .expect("create message");
    let messages = db::sessions::list_messages_by_session(&db, &session_id)
        .await
        .expect("list messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "hello");

    let rust_note_id = db::knowledge::create_note(&db, "Rust", "systems language")
        .await
        .expect("create source note");
    let ghost_note_id = db::knowledge::create_note(&db, "GHOST", "agent project")
        .await
        .expect("create target note");

    let _edge_id = db::knowledge::create_edge(&db, &ghost_note_id, &rust_note_id, "written_in")
        .await
        .expect("create edge");

    let related = db::knowledge::related_note_ids(&db, &ghost_note_id)
        .await
        .expect("query related notes");
    assert_eq!(related, vec![rust_note_id]);

    let rust_note = db::knowledge::get_note(&db, &related[0])
        .await
        .expect("get related note");
    assert_eq!(rust_note.title, "Rust");
}
