mod common;

use ghost::reference_import::{
    ImportConfigJson, ImportProvenance, ensure_update_metadata, import_from_path, read_import_toml,
    update_references, validate_import_metadata_for_repair,
};

const BOOK_METADATA_FILE: &str = "_metadata.json";
const IMPORT_TOML_FILE: &str = "_import.toml";

#[tokio::test]
async fn book_import_writes_repair_critical_metadata_to_import_toml() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);
    let staging = tempfile::tempdir().expect("create staging dir");

    std::fs::write(
        staging.path().join("chapter-01.md"),
        "# Chapter 1\n\nTest content.\n",
    )
    .expect("write chapter");
    std::fs::write(
        staging.path().join(BOOK_METADATA_FILE),
        serde_json::json!({
            "title": "Test Book",
            "authors": ["Example Author"],
            "language": "en",
            "publisher": "Test Press",
            "publication_date": "2024-01-02",
        })
        .to_string(),
    )
    .expect("write metadata");

    let provenance = ImportProvenance {
        source_type: Some("book".to_string()),
        source_url: Some("/tmp/test-book.epub".to_string()),
        paths: vec![],
        extensions: vec![],
        max_depth: None,
        max_pages: None,
        no_ocr: None,
        page_range: None,
        ..Default::default()
    };

    let result = import_from_path(
        &db,
        workspace_path,
        staging.path(),
        "books/test-book",
        &provenance,
        None,
    )
    .await
    .expect("import book references");
    assert_eq!(result.references_created, 1, "should import one chapter");

    let parsed = read_import_toml(workspace_path, "books/test-book").expect("read import toml");
    assert_eq!(parsed.source_type, "book");
    assert_eq!(parsed.source_url, "/tmp/test-book.epub");
    assert_eq!(parsed.title.as_deref(), Some("Test Book"));
    let expected_authors = vec!["Example Author".to_string()];
    assert_eq!(parsed.authors.as_ref(), Some(&expected_authors));
    assert_eq!(parsed.language.as_deref(), Some("en"));
    assert_eq!(parsed.publisher.as_deref(), Some("Test Press"));
    assert_eq!(parsed.publication_date.as_deref(), Some("2024-01-02"));
}

#[tokio::test]
async fn file_import_writes_conversion_settings_to_import_toml() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);
    let staging = tempfile::tempdir().expect("create staging dir");

    std::fs::write(
        staging.path().join("report.md"),
        "# Report\n\nConverted content.\n",
    )
    .expect("write markdown");
    std::fs::write(
        staging.path().join(BOOK_METADATA_FILE),
        serde_json::json!({
            "no_ocr": true,
            "page_range": [2, 7],
        })
        .to_string(),
    )
    .expect("write file metadata");

    let provenance = ImportProvenance {
        source_type: Some("file".to_string()),
        source_url: Some("/tmp/report.pdf".to_string()),
        no_ocr: Some(false),
        page_range: None,
        ..Default::default()
    };

    import_from_path(
        &db,
        workspace_path,
        staging.path(),
        "files/report",
        &provenance,
        None,
    )
    .await
    .expect("import file references");

    let parsed = read_import_toml(workspace_path, "files/report").expect("read import toml");
    assert_eq!(parsed.source_type, "file");
    assert_eq!(parsed.source_url, "/tmp/report.pdf");
    assert_eq!(parsed.no_ocr, Some(true));
    assert_eq!(parsed.page_range, Some((2, 7)));
}

#[tokio::test]
async fn import_from_path_rejects_incomplete_supported_provenance() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);
    let staging = tempfile::tempdir().expect("create staging dir");

    std::fs::write(staging.path().join("page.md"), "# Page\n\nTest content.\n")
        .expect("write page");

    let provenance = ImportProvenance {
        source_type: Some("git".to_string()),
        source_url: Some("https://github.com/example/docs".to_string()),
        git_ref: None,
        paths: vec![],
        extensions: vec![".md".to_string()],
        max_depth: None,
        max_pages: None,
        version_ref: None,
        no_ocr: None,
        page_range: None,
    };
    let topic = "docs/incomplete-provenance";

    let error = import_from_path(
        &db,
        workspace_path,
        staging.path(),
        topic,
        &provenance,
        None,
    )
    .await
    .expect_err("supported imports should reject incomplete provenance");

    assert!(
        error
            .to_string()
            .contains("repair-critical import provenance"),
        "expected explicit provenance failure, got: {error}",
    );

    let topic_dir = workspace_path.join("references").join(topic);
    assert!(
        !topic_dir.exists(),
        "invalid provenance should fail before writing any reference files"
    );
    assert!(
        ghost::db::knowledge::find_topic_by_name(&db, topic)
            .await
            .expect("query topic")
            .is_none(),
        "invalid provenance should fail before creating topic rows"
    );
}

#[test]
fn read_import_toml_parses_supported_source_metadata_shapes() {
    let (_config, workspace, _config_dir) = common::test_workspace();
    let cases = [
        (
            "git/docs",
            r#"
source_type = "git"
source_url = "https://github.com/example/docs"
git_ref = "main"
paths = ["docs/", "guides/"]
extensions = [".md", ".mdx"]
"#,
            ImportConfigJson {
                source_type: "git".to_string(),
                source_url: "https://github.com/example/docs".to_string(),
                git_ref: Some("main".to_string()),
                paths: vec!["docs/".to_string(), "guides/".to_string()],
                extensions: vec![".md".to_string(), ".mdx".to_string()],
                max_depth: None,
                max_pages: None,
                no_ocr: None,
                page_range: None,
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
            },
        ),
        (
            "crawl/docs",
            r#"
source_type = "crawl"
source_url = "https://example.com/docs"
max_depth = 2
max_pages = 25
"#,
            ImportConfigJson {
                source_type: "crawl".to_string(),
                source_url: "https://example.com/docs".to_string(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: Some(2),
                max_pages: Some(25),
                no_ocr: None,
                page_range: None,
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
            },
        ),
        (
            "files/report",
            r#"
source_type = "file"
source_url = "/tmp/report.pdf"
no_ocr = true
page_range = [1, 10]
"#,
            ImportConfigJson {
                source_type: "file".to_string(),
                source_url: "/tmp/report.pdf".to_string(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
                no_ocr: Some(true),
                page_range: Some((1, 10)),
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
            },
        ),
        (
            "books/example",
            r#"
source_type = "book"
source_url = "/tmp/book.epub"
title = "Book Example"
authors = ["Author One", "Author Two"]
language = "en"
publisher = "Books Inc"
publication_date = "2023-09-10"
"#,
            ImportConfigJson {
                source_type: "book".to_string(),
                source_url: "/tmp/book.epub".to_string(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
                no_ocr: None,
                page_range: None,
                title: Some("Book Example".to_string()),
                authors: Some(vec!["Author One".to_string(), "Author Two".to_string()]),
                language: Some("en".to_string()),
                publisher: Some("Books Inc".to_string()),
                publication_date: Some("2023-09-10".to_string()),
            },
        ),
    ];

    for (topic_name, content, expected) in cases {
        let topic_dir = workspace.path().join("references").join(topic_name);
        std::fs::create_dir_all(&topic_dir).expect("create topic dir");
        std::fs::write(topic_dir.join(IMPORT_TOML_FILE), content).expect("write import toml");

        let parsed = read_import_toml(workspace.path(), topic_name).expect("parse import toml");
        assert_eq!(parsed.source_type, expected.source_type);
        assert_eq!(parsed.source_url, expected.source_url);
        assert_eq!(parsed.git_ref, expected.git_ref);
        assert_eq!(parsed.paths, expected.paths);
        assert_eq!(parsed.extensions, expected.extensions);
        assert_eq!(parsed.max_depth, expected.max_depth);
        assert_eq!(parsed.max_pages, expected.max_pages);
        assert_eq!(parsed.no_ocr, expected.no_ocr);
        assert_eq!(parsed.page_range, expected.page_range);
        assert_eq!(parsed.title, expected.title);
        assert_eq!(parsed.authors, expected.authors);
        assert_eq!(parsed.language, expected.language);
        assert_eq!(parsed.publisher, expected.publisher);
        assert_eq!(parsed.publication_date, expected.publication_date);
    }
}

#[test]
fn repair_validation_requires_git_version_ref() {
    let (_config, workspace, _config_dir) = common::test_workspace();
    let topic_name = "git/missing-version-ref";
    let topic_dir = workspace.path().join("references").join(topic_name);
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");
    std::fs::write(
        topic_dir.join(IMPORT_TOML_FILE),
        r#"
source_type = "git"
source_url = "https://github.com/example/docs"
git_ref = "main"
"#,
    )
    .expect("write import toml");

    let error = validate_import_metadata_for_repair(workspace.path(), topic_name)
        .expect_err("repair validation should require git version_ref");
    assert!(
        error.to_string().contains("version_ref"),
        "expected strict repair validation to require version_ref, got: {error}",
    );
}

#[tokio::test]
async fn supported_git_and_crawl_imports_write_reconstructable_import_toml() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);

    let cases = [
        (
            "docs/git-repair",
            ImportProvenance {
                source_type: Some("git".to_string()),
                source_url: Some("https://github.com/example/docs".to_string()),
                version_ref: Some("abc123".to_string()),
                git_ref: Some("main".to_string()),
                paths: vec!["docs/".to_string()],
                extensions: vec![".md".to_string()],
                max_depth: None,
                max_pages: None,
                no_ocr: None,
                page_range: None,
            },
        ),
        (
            "docs/crawl-repair",
            ImportProvenance {
                source_type: Some("crawl".to_string()),
                source_url: Some("https://example.com/docs".to_string()),
                version_ref: None,
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: Some(2),
                max_pages: Some(15),
                no_ocr: None,
                page_range: None,
            },
        ),
    ];

    for (topic, provenance) in cases {
        let staging = tempfile::tempdir().expect("create staging dir");
        std::fs::write(staging.path().join("page.md"), "# Page\n\ncontent\n").expect("write md");

        import_from_path(
            &db,
            workspace_path,
            staging.path(),
            topic,
            &provenance,
            None,
        )
        .await
        .expect("import supported source");

        let parsed = read_import_toml(workspace_path, topic).expect("read import toml");
        assert_eq!(parsed.source_type, provenance.source_type.clone().unwrap());
        assert_eq!(parsed.source_url, provenance.source_url.clone().unwrap());
        assert_eq!(parsed.git_ref, provenance.git_ref.clone());
        assert_eq!(parsed.paths, provenance.paths);
        assert_eq!(parsed.extensions, provenance.extensions);
        assert_eq!(parsed.max_depth, provenance.max_depth);
        assert_eq!(parsed.max_pages, provenance.max_pages);
    }
}

#[tokio::test]
async fn ensure_update_metadata_backfills_import_toml_from_existing_import_batch() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);
    let topic_name = "docs/backfilled-import-metadata";
    let topic_id = ghost::reference_import::ensure_topic_hierarchy(&db, topic_name)
        .await
        .expect("create topic");

    let import_config = ImportConfigJson {
        source_type: "git".to_string(),
        source_url: "https://github.com/example/docs".to_string(),
        git_ref: Some("main".to_string()),
        paths: vec!["docs/".to_string()],
        extensions: vec![".md".to_string()],
        max_depth: None,
        max_pages: None,
        no_ocr: None,
        page_range: None,
        title: None,
        authors: None,
        language: None,
        publisher: None,
        publication_date: None,
    };
    let import_config_json =
        serde_json::to_string(&import_config).expect("serialize import config");
    ghost::db::knowledge::upsert_import_batch(
        &db,
        &topic_id,
        "git",
        "https://github.com/example/docs",
        Some("abc123"),
        7,
        Some(&import_config_json),
    )
    .await
    .expect("create import batch");

    let metadata = ensure_update_metadata(&db, workspace_path, &topic_id, topic_name)
        .await
        .expect("backfill metadata");

    let parsed = read_import_toml(workspace_path, topic_name).expect("read backfilled import toml");
    assert_eq!(metadata.config.source_type, "git");
    assert_eq!(parsed.source_type, "git");
    assert_eq!(parsed.source_url, "https://github.com/example/docs");
    assert_eq!(parsed.git_ref.as_deref(), Some("main"));
    assert_eq!(parsed.paths, vec!["docs/".to_string()]);
    assert_eq!(parsed.extensions, vec![".md".to_string()]);
}

#[tokio::test]
async fn update_references_rejects_insufficient_crawl_import_metadata() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = std::path::Path::new(&config.workspace);
    let topic_name = "docs/incomplete-crawl-import";

    ghost::reference_import::ensure_topic_hierarchy(&db, topic_name)
        .await
        .expect("create topic");

    let topic_dir = workspace_path.join("references").join(topic_name);
    std::fs::create_dir_all(&topic_dir).expect("create topic dir");
    std::fs::write(
        topic_dir.join(IMPORT_TOML_FILE),
        r#"
source_type = "crawl"
source_url = "https://example.com/docs"
"#,
    )
    .expect("write incomplete import toml");

    let error = update_references(&db, workspace_path, topic_name, None)
        .await
        .expect_err("insufficient crawl metadata should fail");

    assert!(
        error
            .to_string()
            .contains("missing repair-critical import metadata"),
        "expected explicit insufficient metadata failure, got: {error}",
    );
    assert!(
        error.to_string().contains("crawl"),
        "expected source-type-specific validation failure, got: {error}",
    );
}
