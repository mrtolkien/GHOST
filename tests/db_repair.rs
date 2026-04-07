mod common;

use std::path::Path;

#[tokio::test]
async fn repair_rebuilds_file_backed_state_and_preserves_db_only_tables_in_dry_run() {
    let (db, config, workspace, _config_dir) = common::test_database().await;
    let workspace_path = workspace.path();

    let session_id = ghost::db::sessions::create_session(&db)
        .await
        .expect("create session");
    let _message_id = ghost::db::sessions::create_message(&db, &session_id, "user", "hello repair")
        .await
        .expect("create message");

    let topic_dir = workspace_path.join("references").join("books/repair-test");
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");
    std::fs::write(topic_dir.join("chapter-01.md"), "# Chapter 1\n\nrepaired\n")
        .expect("write reference");
    std::fs::write(
        topic_dir.join("_import.toml"),
        r#"
source_type = "book"
source_url = "/tmp/repair-test.epub"
title = "Repair Test"
"#,
    )
    .expect("write import metadata");

    let report = Box::pin(ghost::db::repair::repair_database(
        workspace_path,
        config.embeddings.dimension,
        true,
    ))
    .await
    .expect("repair dry-run");

    assert!(
        report.success,
        "dry-run should verify successfully: {report:?}"
    );
    assert!(
        report.candidate_db.exists(),
        "candidate DB should be written"
    );
    assert!(
        report.backup_db.is_none(),
        "dry-run must not swap the live DB"
    );

    let candidate_workspace = tempfile::TempDir::new().expect("candidate workspace");
    std::fs::copy(
        &report.candidate_db,
        candidate_workspace.path().join("ghost.db"),
    )
    .expect("copy candidate db");
    let candidate_db = ghost::db::connect(candidate_workspace.path(), config.embeddings.dimension)
        .await
        .expect("connect candidate db");

    let topic = ghost::db::knowledge::find_topic_by_name(&candidate_db, "books/repair-test")
        .await
        .expect("find topic")
        .expect("topic exists");
    let refs = ghost::db::knowledge::list_references_by_topic(&candidate_db, Some(&topic.id), 10)
        .await
        .expect("list references");
    assert_eq!(refs.len(), 1, "reference should be rebuilt from disk");

    let batch = ghost::db::knowledge::get_import_batch_by_topic(&candidate_db, &topic.id)
        .await
        .expect("get import batch")
        .expect("import batch exists");
    assert_eq!(batch.source_type, "book");
    assert_eq!(batch.source_url, "/tmp/repair-test.epub");

    let copied_session = ghost::db::sessions::get_session(&candidate_db, &session_id)
        .await
        .expect("session copied");
    assert_eq!(copied_session.id, session_id);

    let live_session = ghost::db::sessions::get_session(&db, &session_id)
        .await
        .expect("live session remains");
    assert_eq!(live_session.id, session_id);
}

#[test]
fn repair_artifact_paths_live_beside_workspace_db() {
    let workspace_db = Path::new("/tmp/workspace/ghost.db");
    let stamp = "2026-04-07T16-00-00Z";

    let candidate = ghost::db::repair::candidate_db_path(workspace_db, stamp);
    let report = ghost::db::repair::report_path(workspace_db, stamp);
    let backup = ghost::db::repair::backup_path(workspace_db, stamp);

    assert_eq!(
        candidate,
        Path::new("/tmp/workspace/ghost.db.repair-2026-04-07T16-00-00Z.candidate")
    );
    assert_eq!(
        report,
        Path::new("/tmp/workspace/ghost.db.repair-2026-04-07T16-00-00Z.report.json")
    );
    assert_eq!(
        backup,
        Path::new("/tmp/workspace/ghost.db.backup-2026-04-07T16-00-00Z")
    );
}
