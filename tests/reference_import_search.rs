mod common;

use ghost::db;

// --- Topic hierarchy ---

#[tokio::test]
async fn create_topic_hierarchy() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let leaf_id = ghost::reference_import::ensure_topic_hierarchy(&db, "dioxus/docs")
        .await
        .expect("ensure hierarchy");

    // Both "dioxus" and "dioxus/docs" should exist
    let parent = db::knowledge::find_topic_by_name(&db, "dioxus")
        .await
        .expect("find parent")
        .expect("parent exists");
    let child = db::knowledge::find_topic_by_name(&db, "dioxus/docs")
        .await
        .expect("find child")
        .expect("child exists");

    assert_eq!(child.id, leaf_id);
    assert_ne!(parent.id, child.id);
}

// --- Topic-scoped reference search ---

#[tokio::test]
async fn search_references_scoped_by_topic() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    // Create two topics
    let t1 = db::knowledge::find_or_create_topic(&db, "dioxus/docs")
        .await
        .expect("topic1");
    let t2 = db::knowledge::find_or_create_topic(&db, "sqlx/api")
        .await
        .expect("topic2");

    // Create references under each
    db::knowledge::create_reference(
        &db,
        &t1,
        "dioxus/docs/hooks.md",
        "Hooks are reactive",
        None,
        None,
        None,
    )
    .await
    .expect("ref1");
    db::knowledge::create_reference(
        &db,
        &t2,
        "sqlx/api/pool.md",
        "Connection pool management",
        None,
        None,
        None,
    )
    .await
    .expect("ref2");

    // Search scoped to dioxus/docs
    let hits = db::knowledge::search_references(&db, "hooks", 10, Some(&t1))
        .await
        .expect("search scoped");
    assert!(!hits.is_empty(), "should find hooks reference");
    assert!(
        hits.iter().all(|h| !h.id.is_empty()),
        "hits should have IDs"
    );

    // Search scoped to sqlx/api should NOT find dioxus refs
    let hits = db::knowledge::search_references(&db, "hooks", 10, Some(&t2))
        .await
        .expect("search scoped sqlx");
    assert!(hits.is_empty(), "sqlx topic should not have hooks");
}

// --- Snippet quality ---

#[tokio::test]
async fn reference_search_snippet_contains_matched_term() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let topic_id = db::knowledge::find_or_create_topic(&db, "test-topic")
        .await
        .unwrap();
    db::knowledge::create_reference(
        &db,
        &topic_id,
        "test-topic/rules.md",
        "## Introduction\n\nThis is a long preamble about the game.\n\n## Break Rules\n\nDuring a break, players must discard down to their hand limit.",
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let results = db::knowledge::search_references(&db, "break", 10, None)
        .await
        .unwrap();

    assert!(!results.is_empty(), "should find the reference");
    let snippet = &results[0].snippet;
    assert!(
        snippet.to_lowercase().contains("break"),
        "snippet should contain the matched term 'break', got: {snippet}"
    );
}

// --- Prefix matching ---

#[tokio::test]
async fn prefix_matching_finds_subtopics() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    db::knowledge::find_or_create_topic(&db, "dioxus")
        .await
        .expect("parent");
    db::knowledge::find_or_create_topic(&db, "dioxus/docs")
        .await
        .expect("child1");
    db::knowledge::find_or_create_topic(&db, "dioxus/source")
        .await
        .expect("child2");

    let matches = db::knowledge::find_topics_by_prefix(&db, "dioxus")
        .await
        .expect("prefix search");

    assert_eq!(matches.len(), 3, "should match parent + 2 children");

    let names: Vec<&str> = matches.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"dioxus"));
    assert!(names.contains(&"dioxus/docs"));
    assert!(names.contains(&"dioxus/source"));
}

// --- List topics with counts ---

#[tokio::test]
async fn list_topics_returns_ref_counts() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let tid = db::knowledge::find_or_create_topic(&db, "test-topic")
        .await
        .expect("topic");

    db::knowledge::create_reference(&db, &tid, "test-topic/a.md", "content a", None, None, None)
        .await
        .expect("ref a");
    db::knowledge::create_reference(&db, &tid, "test-topic/b.md", "content b", None, None, None)
        .await
        .expect("ref b");

    let topics = db::knowledge::list_topics(&db).await.expect("list");
    let found = topics
        .iter()
        .find(|t| t.name == "test-topic")
        .expect("find topic");
    assert_eq!(found.ref_count, 2);
}

// --- Delete cascades ---

#[tokio::test]
async fn delete_references_by_topic_cascades() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    let tid = db::knowledge::find_or_create_topic(&db, "ephemeral")
        .await
        .expect("topic");

    db::knowledge::create_reference(&db, &tid, "ephemeral/x.md", "x content", None, None, None)
        .await
        .expect("ref");

    let count_before = db::knowledge::count_references_by_topic(&db, &tid)
        .await
        .expect("count");
    assert_eq!(count_before, 1);

    db::knowledge::delete_references_by_topic(&db, &tid)
        .await
        .expect("delete refs");

    let count_after = db::knowledge::count_references_by_topic(&db, &tid)
        .await
        .expect("count after");
    assert_eq!(count_after, 0);
}

// --- Search topics via BM25 ---

#[tokio::test]
async fn search_topics_bm25() {
    let (db, _config, _workspace, _config_dir) = common::test_database().await;

    db::knowledge::find_or_create_topic(&db, "dioxus")
        .await
        .expect("t1");
    db::knowledge::find_or_create_topic(&db, "dioxus/docs")
        .await
        .expect("t2");
    db::knowledge::find_or_create_topic(&db, "sqlx/api")
        .await
        .expect("t3");

    let hits = db::knowledge::search_topics(&db, "dioxus", 10)
        .await
        .expect("search topics");

    assert!(!hits.is_empty(), "should find dioxus topics");
    assert!(
        hits.iter().all(|h| h.title.contains("dioxus")),
        "all hits should contain dioxus"
    );
}
