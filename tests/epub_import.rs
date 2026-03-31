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
