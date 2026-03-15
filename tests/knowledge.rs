mod common;

use ghost::db;
use ghost::knowledge;
use ghost::tools::{ToolContext, ToolManager};
use serde_json::json;

fn reflection_tools() -> Vec<String> {
    vec![
        "run_shell_command",
        "read_file",
        "write_file",
        "file_edit",
        "todo",
        "knowledge_search",
        "web_search",
        "web_fetch",
        "note_write",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

// --- DB CRUD & field verification ---

#[tokio::test]
async fn create_note_and_retrieve_all_fields() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let id = db::knowledge::create_note_full(
        &db,
        "Rust Language",
        "A systems programming language.",
        &["programming".to_string(), "systems".to_string()],
        &[],
        8,
        None,
        None,
        None,
    )
    .await
    .expect("create note");

    let note = db::knowledge::get_note(&db, &id).await.expect("get note");
    assert_eq!(note.title, "Rust Language");
    assert_eq!(note.body, "A systems programming language.");
    assert_eq!(note.tags_parsed(), vec!["programming", "systems"]);
    assert_eq!(note.trust, 8);
}

#[tokio::test]
async fn update_note_changes_fields() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let id =
        db::knowledge::create_note_full(&db, "Draft", "old body", &[], &[], 5, None, None, None)
            .await
            .expect("create");

    let before = db::knowledge::get_note(&db, &id).await.expect("get before");

    db::knowledge::update_note(
        &db,
        &id,
        "new body",
        &["updated".to_string()],
        &[],
        7,
        None,
        None,
        None,
    )
    .await
    .expect("update");

    let after = db::knowledge::get_note(&db, &id).await.expect("get after");
    assert_eq!(after.body, "new body");
    assert_eq!(after.tags_parsed(), vec!["updated"]);
    assert_eq!(after.trust, 7);
    assert!(after.updated_at >= before.updated_at);
}

// --- Wiki links & edge creation ---

#[tokio::test]
async fn wiki_link_creates_relates_to_edge_and_stub() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let note_id = db::knowledge::create_note_full(
        &db,
        "My Note",
        "See [[Rust]]",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create note");

    let links = knowledge::extract_wiki_links("See [[Rust]]");
    let result = knowledge::reconcile::reconcile_edges(&db, &note_id, "My Note", &links)
        .await
        .expect("reconcile");

    assert_eq!(result.created, 1);
    assert_eq!(result.stubs_created, 1);

    // Verify stub was created
    let stub = db::knowledge::find_note_by_title(&db, "Rust")
        .await
        .expect("find stub")
        .expect("stub exists");
    assert_eq!(stub.body, "");
    assert_eq!(stub.trust, 1);

    // Verify edge exists
    let edges = db::knowledge::outgoing_edges(&db, &note_id)
        .await
        .expect("outgoing");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].label, "relates_to");
    assert_eq!(edges[0].to_id, stub.id);
}

#[tokio::test]
async fn typed_wiki_link_creates_labeled_edge() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let rust_id =
        db::knowledge::create_note_full(&db, "Rust", "A language", &[], &[], 5, None, None, None)
            .await
            .expect("create Rust");

    let note_id = db::knowledge::create_note_full(
        &db,
        "Ghost",
        "Built with [[written_in>Rust]]",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create Ghost");

    let links = knowledge::extract_wiki_links("Built with [[written_in>Rust]]");
    knowledge::reconcile::reconcile_edges(&db, &note_id, "Ghost", &links)
        .await
        .expect("reconcile");

    let edges = db::knowledge::outgoing_edges(&db, &note_id)
        .await
        .expect("outgoing");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].label, "written_in");
    assert_eq!(edges[0].to_id, rust_id);
}

#[tokio::test]
async fn removing_link_deletes_edge() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let _rust_id = db::knowledge::create_note_full(&db, "Rust", "", &[], &[], 5, None, None, None)
        .await
        .expect("create Rust");

    let note_id = db::knowledge::create_note_full(
        &db,
        "My Note",
        "See [[Rust]]",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create note");

    // Initial reconcile creates edge
    let links = knowledge::extract_wiki_links("See [[Rust]]");
    knowledge::reconcile::reconcile_edges(&db, &note_id, "My Note", &links)
        .await
        .expect("reconcile 1");
    assert_eq!(
        db::knowledge::outgoing_edges(&db, &note_id)
            .await
            .expect("edges")
            .len(),
        1
    );

    // Update with no links — edge should be deleted
    let no_links = knowledge::extract_wiki_links("No more links");
    let result = knowledge::reconcile::reconcile_edges(&db, &note_id, "My Note", &no_links)
        .await
        .expect("reconcile 2");
    assert_eq!(result.deleted, 1);
    assert_eq!(
        db::knowledge::outgoing_edges(&db, &note_id)
            .await
            .expect("edges")
            .len(),
        0
    );
}

// --- BM25 search ---

#[tokio::test]
async fn bm25_search_returns_results() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    db::knowledge::create_note_full(
        &db,
        "Rust Programming",
        "Rust is a systems programming language focused on safety.",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create note 1");

    db::knowledge::create_note_full(
        &db,
        "Python",
        "Python is an interpreted language for scripting.",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create note 2");

    let hits = db::knowledge::search_notes(&db, "rust programming", 10)
        .await
        .expect("search");
    assert!(!hits.is_empty(), "should find at least one result");
    assert_eq!(hits[0].title, "Rust Programming");
}

// --- Graph traversal: depth-1 chain ---

#[tokio::test]
async fn graph_chain_neighbors() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let a =
        db::knowledge::create_note_full(&db, "A", "Links to [[B]]", &[], &[], 5, None, None, None)
            .await
            .expect("create A");
    let b =
        db::knowledge::create_note_full(&db, "B", "Links to [[C]]", &[], &[], 5, None, None, None)
            .await
            .expect("create B");
    let c = db::knowledge::create_note_full(&db, "C", "End node", &[], &[], 5, None, None, None)
        .await
        .expect("create C");

    // A -> B
    let links_a = knowledge::extract_wiki_links("Links to [[B]]");
    knowledge::reconcile::reconcile_edges(&db, &a, "A", &links_a)
        .await
        .expect("reconcile A");

    // B -> C
    let links_b = knowledge::extract_wiki_links("Links to [[C]]");
    knowledge::reconcile::reconcile_edges(&db, &b, "B", &links_b)
        .await
        .expect("reconcile B");

    // B should have 1 outgoing (to C) and 1 incoming (from A)
    let outgoing = db::knowledge::outgoing_edges(&db, &b).await.expect("out");
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].to_id, c);

    let incoming = db::knowledge::incoming_edges(&db, &b).await.expect("in");
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].from_id, a);
}

// --- Knowledge write tools via ToolManager ---

#[tokio::test]
async fn note_write_tool_creates_file_and_db_record() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let session_id = db::sessions::create_session(&db)
        .await
        .expect("create session");
    let ctx = ToolContext {
        workspace: config.workspace.clone(),
        cwd: config.workspace.clone(),
        db: db.clone(),
        config: config.clone(),
        session_id: session_id.clone(),
        agent_runner: None,
        event_tx: None,
        channel_id: None,
        confirmation_tx: None,
        browser_session: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
    };
    let manager = ToolManager::for_agent(&reflection_tools());

    let result = manager
        .execute(
            "note_write",
            json!({
                "action": "create",
                "title": "Test Note",
                "body": "This links to [[Concept]]",
                "tags": ["test"],
                "trust": 7
            }),
            &ctx,
        )
        .await
        .expect("note_write create");

    assert!(result.text.contains("Created note 'Test Note'"));
    assert!(result.text.contains("Edges: 1 created"));

    // Verify file on disk (tag "test" → subfolder "test/")
    let note_path = config.workspace.join("notes/test/test_note.md");
    assert!(note_path.exists());

    // Verify DB record
    let note = db::knowledge::find_note_by_title(&db, "Test Note")
        .await
        .expect("find")
        .expect("exists");
    assert_eq!(note.trust, 7);
    assert_eq!(note.tags_parsed(), vec!["test"]);
}

// --- Tags with counts ---

#[tokio::test]
async fn tags_with_correct_counts() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    db::knowledge::create_note_full(
        &db,
        "Note A",
        "body",
        &["rust".to_string(), "systems".to_string()],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create A");

    db::knowledge::create_note_full(
        &db,
        "Note B",
        "body",
        &["rust".to_string(), "web".to_string()],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create B");

    let tags = db::knowledge::list_tags_with_counts(&db)
        .await
        .expect("tags");

    let rust_count = tags.iter().find(|(t, _)| t == "rust").map(|(_, c)| *c);
    let systems_count = tags.iter().find(|(t, _)| t == "systems").map(|(_, c)| *c);
    let web_count = tags.iter().find(|(t, _)| t == "web").map(|(_, c)| *c);

    assert_eq!(rust_count, Some(2));
    assert_eq!(systems_count, Some(1));
    assert_eq!(web_count, Some(1));
}

// --- Recent sorted by updated_at ---

#[tokio::test]
async fn recent_returns_items_sorted() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    db::knowledge::create_note_full(&db, "First Note", "body", &[], &[], 5, None, None, None)
        .await
        .expect("create first");

    // Small delay to ensure different timestamps
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    db::knowledge::create_note_full(&db, "Second Note", "body", &[], &[], 5, None, None, None)
        .await
        .expect("create second");

    let recent = db::knowledge::list_recent(&db, 10).await.expect("recent");
    assert!(recent.len() >= 2);
    // Most recent should come first
    assert_eq!(recent[0].title, "Second Note");
    assert_eq!(recent[1].title, "First Note");
}

// --- Orphan notes ---

#[tokio::test]
async fn orphan_notes_detected() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    // Create a truly isolated note (no edges at all)
    db::knowledge::create_note_full(
        &db,
        "Isolated",
        "No connections",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create isolated");

    let connected_from = db::knowledge::create_note_full(
        &db,
        "Connected",
        "Has an edge",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create connected");

    let target = db::knowledge::create_note_full(
        &db,
        "Target",
        "Receives edge",
        &[],
        &[],
        5,
        None,
        None,
        None,
    )
    .await
    .expect("create target");

    db::knowledge::create_edge(&db, &connected_from, &target, "relates_to")
        .await
        .expect("create edge");

    let orphans = db::knowledge::orphan_notes(&db).await.expect("orphans");
    let orphan_titles: Vec<&str> = orphans.iter().map(|n| n.title.as_str()).collect();

    // Isolated has no edges at all — it's an orphan
    assert!(orphan_titles.contains(&"Isolated"), "Isolated is an orphan");
    // Connected has an outgoing edge — not an orphan
    assert!(!orphan_titles.contains(&"Connected"));
    // Target has an incoming edge — not an orphan
    assert!(!orphan_titles.contains(&"Target"));
}

// --- Cited edges ---

#[tokio::test]
async fn link_cited_edges_creates_note_to_reference_edges() {
    use ghost::knowledge::{NoteFrontMatter, serialize_note};
    use ghost::web::{ClassifiedCacheFile, link_cited_edges};

    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let ws = &config.workspace;

    // Write a note file with a source URL in frontmatter
    let front = NoteFrontMatter {
        title: "Test Product".to_string(),
        tags: vec![],
        sources: vec!["https://example.com/review".to_string()],
        trust: 5,
    };
    let content = serialize_note(&front, "A review of the product.").unwrap();
    let notes_dir = ws.join("notes");
    std::fs::create_dir_all(&notes_dir).unwrap();
    let slug = knowledge::slug_from_title("Test Product");
    std::fs::write(notes_dir.join(format!("{slug}.md")), &content).unwrap();

    // Create matching DB records
    let note_id = db::knowledge::create_note_full(
        &db,
        "Test Product",
        "A review of the product.",
        &[],
        &["https://example.com/review".to_string()],
        5,
        None,
        Some(&format!("notes/{slug}.md")),
        None,
    )
    .await
    .unwrap();

    let topic_id = db::knowledge::find_or_create_topic(&db, "example-com")
        .await
        .unwrap();
    let ref_id = db::knowledge::create_reference(
        &db,
        &topic_id,
        "references/example-com/review.md",
        "Review content",
        Some("https://example.com/review"),
        None,
        None,
    )
    .await
    .unwrap();

    // Create the reference file on disk (curate_references would have moved it here)
    let ref_dir = ws.join("references/example-com");
    std::fs::create_dir_all(&ref_dir).unwrap();
    std::fs::write(ref_dir.join("review.md"), "Review content").unwrap();

    // Simulate a classified file that was moved to references
    let classified = vec![ClassifiedCacheFile {
        filename: "review.md".to_string(),
        url: "https://example.com/review".to_string(),
        cited: true,
        is_search: false,
        preview: Some("Review content".to_string()),
    }];

    let count = link_cited_edges(&db, ws, &classified).await;
    assert_eq!(count, 1, "should create one cited edge");

    // Verify the edge exists via incoming_cited (ref → notes that cite it)
    let citing_notes = db::knowledge::incoming_cited(&db, &ref_id).await.unwrap();
    assert_eq!(citing_notes.len(), 1);
    assert_eq!(citing_notes[0], note_id);
}
