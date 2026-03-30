#![cfg(feature = "live-tests")]

mod common;

use ghost::convert::git::convert_git;
use ghost::db;
use ghost::reference_import::{ImportProvenance, import_from_path};

/// End-to-end test: import -> simulate changes -> update -> verify diff + orphan protection.
///
/// Requires: network access (GitHub clone).
#[tokio::test]
async fn update_git_references_with_diff_and_orphan_protection() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);

    // --- Phase 1a: Convert to staging ---
    let staging_root = workspace_path.join(".staging");
    let convert_result = convert_git(
        &staging_root,
        "https://github.com/DioxusLabs/docsite",
        &["docs-src/0.7/src/tutorial/".to_string()],
        &[".md".to_string()],
        None,
    )
    .await
    .expect("convert git");

    // --- Phase 1b: Import from staging ---
    let provenance = ImportProvenance {
        source_type: Some("git".to_string()),
        source_url: Some("https://github.com/DioxusLabs/docsite".to_string()),
        version_ref: Some(convert_result.version_ref.clone()),
        git_ref: None,
    };

    let import_result = import_from_path(
        &db,
        workspace_path,
        &convert_result.staging_dir,
        "dioxus/docs",
        &provenance,
        None,
    )
    .await
    .expect("initial import");
    assert!(
        import_result.references_created > 0,
        "should create references"
    );

    let topic = db::knowledge::find_topic_by_name(&db, "dioxus/docs")
        .await
        .expect("find topic")
        .expect("topic exists");

    let batch = db::knowledge::get_import_batch_by_topic(&db, &topic.id)
        .await
        .expect("get batch")
        .expect("batch exists");
    let initial_version_ref = batch.version_ref.clone().expect("should have version_ref");
    let initial_ref_count = db::knowledge::count_references_by_topic(&db, &topic.id)
        .await
        .expect("count refs");

    // --- Phase 2: Simulate upstream changes ---

    // 2a. Modify one reference's file_hash to simulate content change
    let refs = db::knowledge::list_references_by_topic(&db, Some(&topic.id), 10_000)
        .await
        .expect("list refs");
    let target_ref = &refs[0];
    // Set file_hash to "stale" so the real content will differ on re-fetch
    sqlx::query("UPDATE reference SET file_hash = 'stale' WHERE id = ?")
        .bind(&target_ref.id)
        .execute(&db)
        .await
        .expect("mark reference as stale");

    // 2b. Insert a fake reference that won't exist upstream (to test deletion)
    let fake_ref_id = db::knowledge::create_reference(
        &db,
        &topic.id,
        "dioxus/docs/deleted-upstream.md",
        "This file was deleted upstream",
        None,
        Some(&batch.id),
        Some("fake-hash"),
    )
    .await
    .expect("create fake reference");

    // Write the fake file to disk too
    let fake_disk_path = workspace_path
        .join("references")
        .join("dioxus/docs/deleted-upstream.md");
    if let Some(parent) = fake_disk_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(&fake_disk_path, "This file was deleted upstream").expect("write fake file");

    // 2c. Create a note that cites the fake reference (to test orphan protection)
    let note_id = db::knowledge::create_note(&db, "Test Note", "References the fake file")
        .await
        .expect("create note");
    db::knowledge::create_cited_edge(&db, &note_id, &fake_ref_id)
        .await
        .expect("create citation edge");

    // --- Phase 3: Run update ---
    let update_result =
        ghost::reference_import::update_references(&db, workspace_path, "dioxus/docs", None)
            .await
            .expect("update references");

    // --- Phase 4: Assertions ---

    // The "stale" reference should have been updated
    assert!(
        update_result.updated >= 1,
        "should update at least 1 reference (the stale one), got {}",
        update_result.updated
    );

    // The fake reference should be orphaned (not deleted, because it's cited)
    assert_eq!(
        update_result.orphaned, 1,
        "should orphan 1 reference (cited fake file)"
    );
    assert_eq!(
        update_result.deleted, 0,
        "should not delete any (the only deletion candidate is cited)"
    );

    // Most references should be unchanged
    assert!(
        update_result.unchanged > 0,
        "should have unchanged references"
    );

    // Version ref should remain the same (same repo, same HEAD)
    assert_eq!(
        update_result.new_version_ref.as_deref(),
        Some(initial_version_ref.as_str()),
        "version ref should match (same repo HEAD)"
    );

    // The orphaned file should be moved to _orphaned/ on disk
    let orphan_path = workspace_path.join("references/dioxus/docs/_orphaned/deleted-upstream.md");
    assert!(
        orphan_path.exists(),
        "orphaned file should be moved to _orphaned/"
    );

    // The original fake file should be gone from its original location
    assert!(
        !fake_disk_path.exists(),
        "original fake file should be removed from original location"
    );

    // The orphaned reference's DB path should be updated
    let orphan_ref =
        db::knowledge::find_reference_by_path(&db, "dioxus/docs/_orphaned/deleted-upstream.md")
            .await
            .expect("find orphaned ref")
            .expect("orphaned ref exists in DB");
    assert_eq!(orphan_ref.id, fake_ref_id, "orphaned ref ID should match");

    // Total ref count should be correct: initial + 0 created - 0 deleted
    // (the orphaned one is still in the DB, just moved)
    let final_count = db::knowledge::count_references_by_topic(&db, &topic.id)
        .await
        .expect("final count");
    assert_eq!(
        final_count, initial_ref_count,
        "total ref count should be unchanged (orphan is moved, not deleted)"
    );

    // --- Phase 5: Re-run update (should short-circuit) ---
    let update_result2 =
        ghost::reference_import::update_references(&db, workspace_path, "dioxus/docs", None)
            .await
            .expect("second update");

    assert_eq!(
        update_result2.updated, 0,
        "second update should not update anything"
    );
    assert_eq!(
        update_result2.created, 0,
        "second update should not create anything"
    );
}
