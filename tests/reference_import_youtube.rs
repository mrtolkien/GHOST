mod common;

use std::path::Path;

use ghost::db;
use ghost::reference_import::{
    ImportConfigJson, ImportError, ImportProvenance, YoutubeImportProvenance, import_from_path,
    read_import_toml, update_references,
};

#[tokio::test]
async fn youtube_import_persists_provenance_and_links_batch() {
    let (db, config, workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);

    let staging_dir = workspace.path().join(".staging").join("youtube-import");
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");
    std::fs::write(
        staging_dir.join("video.md"),
        "# Transcript\n\nThis is a test transcript.",
    )
    .expect("write staging file");

    let provenance = ImportProvenance {
        source_type: Some("youtube".to_string()),
        source_url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string()),
        youtube: Some(YoutubeImportProvenance {
            video_id: Some("dQw4w9WgXcQ".to_string()),
            title: Some("Never Gonna Give You Up".to_string()),
            channel: Some("Test Channel".to_string()),
            published_at: Some("2024-05-01T12:34:56Z".to_string()),
            duration_seconds: Some(1234),
            transcript_source: Some("auto".to_string()),
            section_count: Some(3),
            chapter_count: Some(1),
            language: Some("en".to_string()),
        }),
        ..Default::default()
    };

    let result = import_from_path(
        &db,
        workspace_path,
        &staging_dir,
        "videos/test",
        &provenance,
        None,
    )
    .await
    .expect("import youtube reference");
    assert_eq!(result.references_created, 1);
    assert_eq!(result.references_skipped, 0);

    let topic = db::knowledge::find_topic_by_name(&db, "videos/test")
        .await
        .expect("find topic")
        .expect("topic should exist");

    let batch = db::knowledge::get_import_batch_by_topic(&db, &topic.id)
        .await
        .expect("get batch")
        .expect("batch should exist");
    assert_eq!(result.batch_id.as_deref(), Some(batch.id.as_str()));

    let import_config: ImportConfigJson =
        serde_json::from_str(batch.import_config.as_deref().expect("import_config json"))
            .expect("parse import_config json");
    assert_eq!(import_config.source_type, "youtube");
    assert_eq!(
        import_config.title.as_deref(),
        Some("Never Gonna Give You Up")
    );
    assert_eq!(import_config.video_id.as_deref(), Some("dQw4w9WgXcQ"));
    assert_eq!(import_config.channel.as_deref(), Some("Test Channel"));
    assert_eq!(
        import_config.published_at.as_deref(),
        Some("2024-05-01T12:34:56Z")
    );
    assert_eq!(import_config.duration_seconds, Some(1234));
    assert_eq!(import_config.transcript_source.as_deref(), Some("auto"));
    assert_eq!(import_config.section_count, Some(3));
    assert_eq!(import_config.chapter_count, Some(1));
    assert_eq!(import_config.language.as_deref(), Some("en"));

    let toml_config = read_import_toml(workspace_path, "videos/test").expect("read _import.toml");
    assert_eq!(toml_config.source_type, "youtube");
    assert_eq!(
        toml_config.title.as_deref(),
        Some("Never Gonna Give You Up")
    );
    assert_eq!(toml_config.video_id.as_deref(), Some("dQw4w9WgXcQ"));

    let refs = db::knowledge::list_references_by_topic(&db, Some(&topic.id), 10)
        .await
        .expect("list refs");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].import_batch_id.as_deref(), Some(batch.id.as_str()));
    assert_eq!(
        refs[0].source_url.as_deref(),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
}

#[tokio::test]
async fn youtube_reimport_backfills_batch_for_existing_references() {
    let (db, config, workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);

    let staging_dir = workspace.path().join(".staging").join("youtube-reimport");
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");
    std::fs::write(
        staging_dir.join("video.md"),
        "# Transcript\n\nThis is a test transcript.",
    )
    .expect("write staging file");

    let provenance = ImportProvenance {
        source_type: Some("youtube".to_string()),
        source_url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string()),
        youtube: Some(YoutubeImportProvenance {
            video_id: Some("dQw4w9WgXcQ".to_string()),
            title: Some("Never Gonna Give You Up".to_string()),
            channel: Some("Test Channel".to_string()),
            published_at: Some("2024-05-01T12:34:56Z".to_string()),
            duration_seconds: Some(1234),
            transcript_source: Some("auto".to_string()),
            section_count: Some(3),
            chapter_count: Some(1),
            language: Some("en".to_string()),
        }),
        ..Default::default()
    };

    let initial = import_from_path(
        &db,
        workspace_path,
        &staging_dir,
        "videos/reimport",
        &Default::default(),
        None,
    )
    .await
    .expect("initial import reference");
    assert_eq!(initial.references_created, 1);
    assert_eq!(initial.references_skipped, 0);

    let result = import_from_path(
        &db,
        workspace_path,
        &staging_dir,
        "videos/reimport",
        &provenance,
        None,
    )
    .await
    .expect("re-import youtube reference");
    assert_eq!(result.references_created, 0);
    assert_eq!(result.references_skipped, 1);

    let topic = db::knowledge::find_topic_by_name(&db, "videos/reimport")
        .await
        .expect("find topic")
        .expect("topic should exist");
    let batch = db::knowledge::get_import_batch_by_topic(&db, &topic.id)
        .await
        .expect("get batch")
        .expect("batch should exist");

    let refs = db::knowledge::list_references_by_topic(&db, Some(&topic.id), 10)
        .await
        .expect("list refs");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].import_batch_id.as_deref(), Some(batch.id.as_str()));
    assert_eq!(
        refs[0].source_url.as_deref(),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
}

#[tokio::test]
async fn youtube_update_references_returns_unsupported_source_error() {
    let (db, config, workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);

    let staging_dir = workspace.path().join(".staging").join("youtube-update");
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");
    std::fs::write(
        staging_dir.join("video.md"),
        "# Transcript\n\nThis is a test transcript.",
    )
    .expect("write staging file");

    let provenance = ImportProvenance {
        source_type: Some("youtube".to_string()),
        source_url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string()),
        youtube: Some(YoutubeImportProvenance {
            video_id: Some("dQw4w9WgXcQ".to_string()),
            title: Some("Never Gonna Give You Up".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    import_from_path(
        &db,
        workspace_path,
        &staging_dir,
        "videos/update-test",
        &provenance,
        None,
    )
    .await
    .expect("import youtube reference");

    let err = update_references(&db, workspace_path, "videos/update-test", None)
        .await
        .expect_err("youtube update should be unsupported");

    match err {
        ImportError::Config(msg) => {
            assert!(msg.contains("unsupported source_type for update: youtube"));
        }
        other => panic!("expected ImportError::Config, got {other:?}"),
    }
}
