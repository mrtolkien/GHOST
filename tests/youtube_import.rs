mod common;

use std::path::{Path, PathBuf};

use ghost::convert::youtube::{TranscriptSource, YoutubeMetadata, YoutubeStagingMetadata};
use ghost::reference_import::{
    ImportProvenance, YoutubeImportProvenance, import_from_path, read_import_toml,
};

fn youtube_provenance() -> ImportProvenance {
    ImportProvenance {
        source_type: Some("youtube".to_string()),
        source_url: Some("https://www.youtube.com/watch?v=test123".to_string()),
        youtube: Some(YoutubeImportProvenance {
            video_id: Some("test123".to_string()),
            title: Some("Test Video".to_string()),
            channel: Some("Example Channel".to_string()),
            published_at: Some("2024-01-02".to_string()),
            duration_seconds: Some(1_234),
            transcript_source: Some("auto".to_string()),
            section_count: Some(2),
            chapter_count: Some(1),
            language: Some("en".to_string()),
        }),
        ..Default::default()
    }
}

fn write_youtube_staging_dir(workspace: &Path) -> PathBuf {
    let staging_dir = workspace.join(".staging").join("youtube-import");
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");

    std::fs::write(
        staging_dir.join("01-0000-intro.md"),
        "# Intro\n\nThis is the first section.",
    )
    .expect("write intro section");
    std::fs::write(
        staging_dir.join("02-0840-main.md"),
        "# Main\n\nThis is the second section.",
    )
    .expect("write main section");
    let metadata = YoutubeStagingMetadata {
        metadata: YoutubeMetadata {
            source_url: "https://www.youtube.com/watch?v=test123".to_string(),
            video_id: "test123".to_string(),
            title: Some("Test Video".to_string()),
            channel: Some("Example Channel".to_string()),
            published_at: Some("2024-01-02".to_string()),
            duration_seconds: Some(1_234),
            language: Some("en".to_string()),
            transcript_source: TranscriptSource::Auto,
        },
        section_count: 2,
        chapter_count: 1,
    };
    std::fs::write(
        staging_dir.join("_metadata.json"),
        serde_json::to_string_pretty(&metadata).expect("serialize staging metadata"),
    )
    .expect("write staging metadata");

    staging_dir
}

#[tokio::test]
async fn youtube_staging_import_writes_metadata_and_import_toml() {
    let (db, config, workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);
    let staging_dir = write_youtube_staging_dir(workspace.path());

    let result = import_from_path(
        &db,
        workspace_path,
        &staging_dir,
        "videos/test-video",
        &youtube_provenance(),
        None,
    )
    .await
    .expect("import youtube staging");

    assert_eq!(result.references_created, 2);
    assert_eq!(result.references_skipped, 0);

    let topic = ghost::db::knowledge::find_topic_by_name(&db, "videos/test-video")
        .await
        .expect("find topic")
        .expect("topic should exist");
    let batch = ghost::db::knowledge::get_import_batch_by_topic(&db, &topic.id)
        .await
        .expect("get batch")
        .expect("batch should exist");

    let import_config = serde_json::from_str::<ghost::reference_import::ImportConfigJson>(
        batch.import_config.as_deref().expect("import config json"),
    )
    .expect("parse import config");
    assert_eq!(import_config.source_type, "youtube");
    assert_eq!(import_config.video_id.as_deref(), Some("test123"));
    assert_eq!(import_config.channel.as_deref(), Some("Example Channel"));
    assert_eq!(import_config.published_at.as_deref(), Some("2024-01-02"));
    assert_eq!(import_config.duration_seconds, Some(1_234));
    assert_eq!(import_config.transcript_source.as_deref(), Some("auto"));
    assert_eq!(import_config.section_count, Some(2));
    assert_eq!(import_config.chapter_count, Some(1));
    assert_eq!(import_config.language.as_deref(), Some("en"));

    let import_toml =
        read_import_toml(workspace_path, "videos/test-video").expect("read youtube _import.toml");
    assert_eq!(import_toml.source_type, "youtube");
    assert_eq!(import_toml.video_id.as_deref(), Some("test123"));
    assert_eq!(import_toml.transcript_source.as_deref(), Some("auto"));
    assert_eq!(import_toml.section_count, Some(2));
    assert_eq!(import_toml.chapter_count, Some(1));

    let refs = ghost::db::knowledge::list_references_by_topic(&db, Some(&topic.id), 10)
        .await
        .expect("list refs");
    assert_eq!(refs.len(), 2);
    assert!(
        refs.iter()
            .all(|reference| reference.import_batch_id.as_deref() == Some(batch.id.as_str()))
    );
    assert!(refs.iter().all(|reference| {
        reference.source_url.as_deref() == Some("https://www.youtube.com/watch?v=test123")
    }));
}

#[tokio::test]
async fn youtube_staging_metadata_populates_cli_style_import_provenance() {
    let (db, config, workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);
    let staging_dir = write_youtube_staging_dir(workspace.path());

    let result = import_from_path(
        &db,
        workspace_path,
        &staging_dir,
        "videos/cli-style",
        &ImportProvenance {
            source_type: Some("youtube".to_string()),
            source_url: Some("https://www.youtube.com/watch?v=test123".to_string()),
            ..ImportProvenance::default()
        },
        None,
    )
    .await
    .expect("import youtube staging with converter metadata");

    assert_eq!(result.references_created, 2);

    let topic = ghost::db::knowledge::find_topic_by_name(&db, "videos/cli-style")
        .await
        .expect("find topic")
        .expect("topic should exist");
    let batch = ghost::db::knowledge::get_import_batch_by_topic(&db, &topic.id)
        .await
        .expect("get batch")
        .expect("batch should exist");

    let import_config = serde_json::from_str::<ghost::reference_import::ImportConfigJson>(
        batch.import_config.as_deref().expect("import config json"),
    )
    .expect("parse import config");
    assert_eq!(import_config.title.as_deref(), Some("Test Video"));
    assert_eq!(import_config.video_id.as_deref(), Some("test123"));
    assert_eq!(import_config.channel.as_deref(), Some("Example Channel"));
    assert_eq!(import_config.transcript_source.as_deref(), Some("auto"));
    assert_eq!(import_config.section_count, Some(2));
    assert_eq!(import_config.chapter_count, Some(1));

    let import_toml =
        read_import_toml(workspace_path, "videos/cli-style").expect("read youtube _import.toml");
    assert_eq!(import_toml.title.as_deref(), Some("Test Video"));
    assert_eq!(import_toml.video_id.as_deref(), Some("test123"));
    assert_eq!(import_toml.channel.as_deref(), Some("Example Channel"));
    assert_eq!(import_toml.transcript_source.as_deref(), Some("auto"));
    assert_eq!(import_toml.section_count, Some(2));
    assert_eq!(import_toml.chapter_count, Some(1));
}

#[cfg(feature = "live-tests")]
#[tokio::test]
async fn youtube_convert_live_from_env_url() {
    let Ok(url) = std::env::var("GHOST_TEST_YOUTUBE_URL") else {
        eprintln!("skipping youtube live test: GHOST_TEST_YOUTUBE_URL not set");
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let result = ghost::convert::youtube::convert_youtube(tmp.path(), &url)
        .await
        .expect("convert_youtube should succeed");

    assert!(result.section_count >= 1);
    assert!(result.staging_dir.exists());
}
