mod common;

use std::path::Path;

use ghost::convert::epub::convert_epub;
use ghost::db;
use ghost::reference_import::{ImportProvenance, import_from_path};

/// Test EPUB path — Animal Farm by George Orwell.
///
/// 13 spine items: titlepage, title page, 11 chapters.
/// Some trivial items are filtered, so expect >= 10 chapters.
const TEST_EPUB: &str = "/home/tolki/Documents/books/Animal Farm.epub";

/// Full pipeline: EPUB → staging (per-chapter markdown) → reference import → DB.
#[tokio::test]
async fn epub_convert_and_import() {
    if !Path::new(TEST_EPUB).exists() {
        eprintln!("skipping epub test: {TEST_EPUB} not found");
        return;
    }

    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);

    // --- Phase 1: Convert EPUB to staging ---

    let staging_root = workspace_path.join(".staging");
    let result =
        convert_epub(&staging_root, Path::new(TEST_EPUB)).expect("convert_epub should succeed");

    // Metadata assertions
    assert_eq!(
        result.metadata.title.as_deref(),
        Some("Animal Farm"),
        "title should be 'Animal Farm'"
    );
    assert!(
        result.metadata.authors.iter().any(|a| a.contains("Orwell")),
        "authors should contain Orwell, got: {:?}",
        result.metadata.authors
    );

    // Chapter count: 13 spine items minus trivial ones
    assert!(
        result.chapter_count >= 10,
        "should have >= 10 chapters, got {}",
        result.chapter_count
    );

    // Staging dir should exist with numbered markdown files
    assert!(result.staging_dir.exists(), "staging dir should exist");
    let md_files: Vec<_> = std::fs::read_dir(&result.staging_dir)
        .expect("read staging dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect();
    assert_eq!(
        md_files.len(),
        result.chapter_count,
        "markdown file count should match chapter_count"
    );

    // Original EPUB should be preserved
    let originals = result.staging_dir.join("_originals");
    assert!(originals.exists(), "_originals dir should exist");
    let original_files: Vec<_> = std::fs::read_dir(&originals)
        .expect("read originals")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(original_files.len(), 1, "should have one original file");

    // _metadata.json should exist
    let metadata_path = result.staging_dir.join("_metadata.json");
    assert!(metadata_path.exists(), "_metadata.json should exist");

    // Read a chapter and verify it contains recognizable content
    let mut found_content = false;
    for entry in &md_files {
        let content = std::fs::read_to_string(entry.path()).expect("read chapter");
        if content.contains("Manor Farm") || content.contains("Old Major") {
            found_content = true;
            break;
        }
    }
    assert!(
        found_content,
        "at least one chapter should contain 'Manor Farm' or 'Old Major'"
    );

    // No chapter should start with the book title as first line
    for entry in &md_files {
        let content = std::fs::read_to_string(entry.path()).expect("read chapter");
        let first_line = content.lines().next().unwrap_or("");
        let bare = first_line.trim_start_matches('#').trim();
        assert_ne!(
            bare.to_lowercase(),
            "animal farm",
            "chapter {} should not start with book title",
            entry.file_name().to_string_lossy()
        );
    }

    // --- Phase 2: Import from staging ---

    let topic = "books/animal-farm";
    let provenance = ImportProvenance {
        source_type: Some("book".to_string()),
        source_url: Some(TEST_EPUB.to_string()),
        version_ref: None,
        git_ref: None,
    };

    let import_result = import_from_path(
        &db,
        workspace_path,
        &result.staging_dir,
        topic,
        &provenance,
        None,
    )
    .await
    .expect("import_from_path should succeed");

    assert!(
        import_result.references_created >= 10,
        "should create >= 10 references, got {}",
        import_result.references_created
    );
    assert_eq!(
        import_result.references_skipped, 0,
        "first import should skip nothing"
    );

    // References should exist on disk
    let ref_dir = workspace_path.join("references").join(topic);
    assert!(ref_dir.exists(), "reference dir should exist on disk");

    // References should exist in DB with correct topic
    let db_topic = db::knowledge::find_topic_by_name(&db, topic)
        .await
        .expect("find topic")
        .expect("topic should exist");

    let ref_count = db::knowledge::count_references_by_topic(&db, &db_topic.id)
        .await
        .expect("count refs");
    assert_eq!(
        ref_count as usize, import_result.references_created,
        "DB ref count should match created count"
    );

    // Parent topic "books" should also exist
    let parent = db::knowledge::find_topic_by_name(&db, "books")
        .await
        .expect("find parent")
        .expect("parent topic 'books' should exist");
    assert_ne!(parent.id, db_topic.id);

    // Import batch should exist with source_type = "book"
    let batch = db::knowledge::get_import_batch_by_topic(&db, &db_topic.id)
        .await
        .expect("get batch")
        .expect("import batch should exist");
    assert_eq!(batch.source_type, "book");

    // _import.toml should exist on disk
    let import_toml = ref_dir.join("_import.toml");
    assert!(import_toml.exists(), "_import.toml should exist");
    let toml_content = std::fs::read_to_string(&import_toml).expect("read _import.toml");
    assert!(
        toml_content.contains("source_type = \"book\""),
        "_import.toml should contain source_type = book"
    );

    // Topic index note should exist
    let note_path = workspace_path.join("notes/books/animal-farm/index.md");
    assert!(note_path.exists(), "topic index note should exist");
}

/// Full pipeline including agent note creation.
/// Requires LLM API access — gated behind live-tests-llms.
#[cfg(feature = "live-tests-llms")]
#[tokio::test]
async fn epub_agent_creates_notes() {
    use std::collections::HashMap;
    use std::time::Duration;

    if !Path::new(TEST_EPUB).exists() {
        eprintln!("skipping epub agent test: {TEST_EPUB} not found");
        return;
    }

    let env = common::live_test_database("epub_agent").await;
    let workspace_path = env.workspace_path();

    // --- Convert + Import ---

    let staging_root = workspace_path.join(".staging");
    let convert_result = convert_epub(&staging_root, Path::new(TEST_EPUB)).expect("convert_epub");

    let topic = "books/animal-farm";
    let provenance = ImportProvenance {
        source_type: Some("book".to_string()),
        source_url: Some(TEST_EPUB.to_string()),
        version_ref: None,
        git_ref: None,
    };

    let import_result = import_from_path(
        &env.db,
        workspace_path,
        &convert_result.staging_dir,
        topic,
        &provenance,
        None,
    )
    .await
    .expect("import_from_path");

    env.log(format!(
        "imported {} references for {topic}",
        import_result.references_created
    ));

    // --- Run book-import agent ---

    let mut args = HashMap::new();
    args.insert("topic".into(), topic.into());
    args.insert("title".into(), "Animal Farm".into());
    args.insert("authors".into(), "George Orwell".into());

    let agent_result = tokio::time::timeout(
        Duration::from_secs(600),
        Box::pin(env.agent_runner.run_with_args("book-import", args, None)),
    )
    .await
    .expect("agent should complete within 10 minutes")
    .expect("agent should succeed");

    env.log(format!("agent session: {}", agent_result.session_id));
    env.log(format!("findings: {}", agent_result.findings));

    // --- Verify notes were created ---

    // Search for a source note about Animal Farm
    let source_hits = db::knowledge::search_notes(&env.db, "Animal Farm", 10, Some("source"))
        .await
        .expect("search source notes");

    assert!(
        !source_hits.is_empty(),
        "should have a source note about Animal Farm. Found 0 hits."
    );

    // Fetch full note record to inspect body
    let source_note = db::knowledge::get_note(&env.db, &source_hits[0].id)
        .await
        .expect("get source note");

    // Source note should mention key themes/elements
    let has_thematic_content = source_note.body.contains("allegory")
        || source_note.body.contains("satire")
        || source_note.body.contains("totalitarian")
        || source_note.body.contains("revolution")
        || source_note.body.contains("power")
        || source_note.body.contains("corruption");
    assert!(
        has_thematic_content,
        "source note should mention themes \
         (allegory/satire/totalitarianism/revolution/power/corruption)"
    );

    // Search for an author note about George Orwell
    let entity_hits = db::knowledge::search_notes(&env.db, "George Orwell", 10, Some("entity"))
        .await
        .expect("search entity notes");

    let orwell_hit = entity_hits.iter().find(|h| h.title.contains("Orwell"));
    assert!(
        orwell_hit.is_some(),
        "should have an entity note about George Orwell. Found: {:?}",
        entity_hits.iter().map(|h| &h.title).collect::<Vec<_>>()
    );

    // Fetch full note to check body
    let author_note = db::knowledge::get_note(&env.db, &orwell_hit.expect("orwell hit").id)
        .await
        .expect("get author note");
    assert!(
        author_note.body.contains("Animal Farm"),
        "author note should mention Animal Farm"
    );
}
