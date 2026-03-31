mod common;

use std::path::Path;
use std::sync::Arc;

/// Test with MockProvider: Docling converts the lotion PDF, quality check flags
/// the page as bad, vision fallback is invoked with a MockProvider, and the
/// mock response text appears in the final markdown.
///
/// Requires `pdftoppm` (poppler-utils) on PATH for page rendering.
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn hybrid_extraction_calls_vision_for_bad_page() {
    use ghost::config::DoclingConfig;
    use ghost::docling::{ConvertOptions, DoclingSource, convert_hybrid};
    use ghost::providers::types::{ChatResponse, ContentBlock, StopReason, Usage};

    let mock_text = "Mock vision extraction: ル・マイルド化粧水".to_string();

    let mock_response = ChatResponse {
        content: vec![ContentBlock::Text {
            text: mock_text.clone(),
        }],
        usage: Usage::default(),
        stop_reason: StopReason::EndTurn,
        model: "mock-vision".to_string(),
        response_id: None,
        turn_state: None,
    };

    let mock = common::MockProvider::new(vec![mock_response]);
    let requests = mock.requests();
    let provider: Arc<dyn ghost::providers::Provider> = Arc::new(mock);

    let workspace_dir = tempfile::tempdir().expect("create workspace tempdir");

    // Use the HTTP backend (docling-serve) so we don't need convert.py locally.
    let docling_url = std::env::var("DOCLING_URL").expect("DOCLING_URL must be set for live tests");
    let docling_config = DoclingConfig {
        url: Some(docling_url),
        timeout: 600,
    };

    let pdf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lotion.pdf");
    assert!(
        pdf_path.exists(),
        "test fixture lotion.pdf must exist at {}",
        pdf_path.display()
    );

    let result = convert_hybrid(
        &docling_config,
        workspace_dir.path(),
        DoclingSource::File { path: &pdf_path },
        &ConvertOptions::default(),
        Some(&provider),
        Some("mock-vision"),
    )
    .await;

    let markdown = result.expect("convert_hybrid should succeed");

    // The mock provider should have been called exactly once (one bad page).
    let reqs = requests.lock().expect("lock requests");
    assert_eq!(
        reqs.len(),
        1,
        "expected exactly 1 vision call, got {}",
        reqs.len()
    );

    // The request should contain an Image content block.
    let has_image = reqs[0].messages.iter().any(|msg| {
        msg.content
            .iter()
            .any(|block| matches!(block, ContentBlock::Image { .. }))
    });
    assert!(has_image, "vision request must contain an Image block");

    // The final markdown should include the mock's response text.
    assert!(
        markdown.contains(&mock_text),
        "final markdown should contain mock text.\nMarkdown:\n{markdown}"
    );
}

/// Full end-to-end test with a real vision model. Verifies that the extracted
/// text actually contains readable Japanese content from the lotion PDF.
#[cfg(feature = "live-tests-llms")]
#[tokio::test]
async fn hybrid_extraction_produces_readable_text() {
    use ghost::docling::{ConvertOptions, DoclingSource, convert_hybrid};
    use ghost::providers::provider_for_alias;

    let config = ghost::config::load().expect("load config");
    let workspace = Path::new(&config.workspace);

    let vision_alias = config
        .models
        .vision
        .as_deref()
        .unwrap_or(&config.models.default);

    let vision_provider =
        provider_for_alias(&config, Some(vision_alias)).expect("vision provider should resolve");
    let vision_model = config
        .models
        .aliases
        .get(vision_alias)
        .map(|m| m.model.clone())
        .expect("vision alias should have a model config");

    let pdf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lotion.pdf");
    assert!(
        pdf_path.exists(),
        "test fixture lotion.pdf must exist at {}",
        pdf_path.display()
    );

    eprintln!("Starting hybrid extraction with vision model: {vision_model}");
    let start = std::time::Instant::now();

    let markdown = convert_hybrid(
        &config.docling,
        workspace,
        DoclingSource::File { path: &pdf_path },
        &ConvertOptions::default(),
        Some(&vision_provider),
        Some(&vision_model),
    )
    .await
    .expect("convert_hybrid should succeed");

    let elapsed = start.elapsed();
    eprintln!(
        "Hybrid extraction completed in {:.1}s, {} chars",
        elapsed.as_secs_f64(),
        markdown.len()
    );
    eprintln!("--- Extracted markdown ---\n{markdown}\n--- End ---");

    // The lotion PDF is a Japanese skincare product. The vision model should
    // extract recognizable strings that do NOT appear in Docling's garbled output.
    // "化粧水" appears even in the garbled OCR, so we check for longer strings
    // that require successful vision extraction.
    let expected_fragments = [
        "ヘパリン類似物質", // key ingredient (heparinoid)
        "健栄製薬",         // manufacturer name
        "グリチルリチン",   // active ingredient
        "ル・マイルド",     // product name in full
    ];

    let found_any = expected_fragments
        .iter()
        .any(|frag| markdown.contains(frag));

    assert!(
        found_any,
        "expected at least one of {expected_fragments:?} in the extracted markdown.\n\
         This indicates the vision model failed to extract Japanese text.",
    );
}

/// Full pipeline test: convert_pdf → import_from_path → verify DB references.
///
/// Exercises the entire new two-step flow with a real PDF, real docling,
/// real vision model, and real DB writes. This is the test that would have
/// caught the poppler_utils nix invocation bug.
///
/// Requires: docling-serve (DOCLING_URL), nix + poppler-utils, vision LLM.
#[cfg(feature = "live-tests-llms")]
#[tokio::test]
async fn convert_pdf_then_import_full_pipeline() {
    use ghost::convert::pdf::{PdfConvertResult, VisionFallback, convert_pdf};
    use ghost::db;
    use ghost::providers::provider_for_alias;
    use ghost::reference_import::{ImportProvenance, import_from_path};

    let config = ghost::config::load().expect("load config");
    let workspace_path = Path::new(&config.workspace);
    let connect_db = db::connect(&config.workspace, config.embeddings.dimension)
        .await
        .expect("connect db");

    // Resolve vision provider
    let vision_alias = config
        .models
        .vision
        .as_deref()
        .unwrap_or(&config.models.default);
    let vision_provider = provider_for_alias(&config, Some(vision_alias)).expect("vision provider");
    let vision_model = config
        .models
        .aliases
        .get(vision_alias)
        .map(|m| m.model.clone())
        .expect("vision model config");

    let pdf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lotion.pdf");
    assert!(pdf_path.exists(), "lotion.pdf fixture must exist");

    // --- Step 1: convert_pdf ---
    let staging_root = tempfile::tempdir().expect("staging tempdir");
    eprintln!("Converting PDF with vision model: {vision_model}");
    let start = std::time::Instant::now();

    let PdfConvertResult {
        staging_dir,
        markdown_file,
    } = convert_pdf(
        staging_root.path(),
        &pdf_path,
        workspace_path,
        &config.docling,
        false, // OCR enabled
        None,  // all pages
        Some(VisionFallback {
            provider: vision_provider,
            model: vision_model,
        }),
    )
    .await
    .expect("convert_pdf should succeed");

    let elapsed = start.elapsed();
    eprintln!("convert_pdf completed in {:.1}s", elapsed.as_secs_f64());

    // Verify staging output
    assert!(staging_dir.exists(), "staging dir should exist");
    let md_path = staging_dir.join(&markdown_file);
    assert!(md_path.exists(), "markdown file should exist in staging");
    let md_content = std::fs::read_to_string(&md_path).expect("read markdown");
    assert!(
        !md_content.is_empty(),
        "converted markdown should not be empty"
    );
    eprintln!(
        "Markdown: {} chars, file: {markdown_file}",
        md_content.len()
    );

    // Verify _originals/ preserved
    let originals = staging_dir.join("_originals");
    assert!(originals.is_dir(), "_originals/ should exist in staging");
    let original_files: Vec<_> = std::fs::read_dir(&originals)
        .expect("read _originals")
        .flatten()
        .collect();
    assert_eq!(
        original_files.len(),
        1,
        "should have exactly 1 original file"
    );

    // Verify content quality (vision model should have extracted Japanese text)
    let expected_fragments = [
        "ヘパリン類似物質",
        "健栄製薬",
        "グリチルリチン",
        "ル・マイルド",
    ];
    let found_any = expected_fragments
        .iter()
        .any(|frag| md_content.contains(frag));
    assert!(
        found_any,
        "expected Japanese text in converted markdown — vision fallback may have failed"
    );

    // --- Step 2: import_from_path ---
    let topic = "test/lotion-pdf-pipeline";
    let provenance = ImportProvenance {
        source_type: Some("file".to_string()),
        source_url: Some(pdf_path.display().to_string()),
        ..Default::default()
    };

    let result = import_from_path(
        &connect_db,
        workspace_path,
        &staging_dir,
        topic,
        &provenance,
        None,
    )
    .await
    .expect("import_from_path should succeed");

    assert_eq!(
        result.references_created, 1,
        "should create exactly 1 reference"
    );
    assert_eq!(result.references_skipped, 0, "should skip nothing");

    // Verify DB record
    let refs = db::knowledge::list_references_by_topic(&connect_db, Some(&result.topic_id), 10)
        .await
        .expect("list refs");
    assert_eq!(refs.len(), 1, "should have 1 reference in DB");

    // Verify reference file on disk
    let ref_on_disk = workspace_path
        .join("references")
        .join(topic)
        .join(&markdown_file);
    assert!(ref_on_disk.exists(), "reference file should exist on disk");

    // Verify _originals/ copied to references
    let ref_originals = workspace_path
        .join("references")
        .join(topic)
        .join("_originals");
    assert!(
        ref_originals.is_dir(),
        "_originals/ should be copied to references"
    );

    // Verify _import.toml
    let import_toml = workspace_path
        .join("references")
        .join(topic)
        .join("_import.toml");
    assert!(import_toml.exists(), "_import.toml should be written");
    let toml_content = std::fs::read_to_string(&import_toml).expect("read _import.toml");
    assert!(
        toml_content.contains("source_type = \"file\""),
        "_import.toml should record file source type"
    );

    // --- Step 3: Idempotent re-import ---
    // Re-create staging (the first one may have been cleaned up)
    let staging2 = tempfile::tempdir().expect("staging2");
    let staging2_dir = staging2.path().join("lotion-reimport");
    std::fs::create_dir_all(&staging2_dir).expect("create staging2 dir");
    std::fs::write(staging2_dir.join(&markdown_file), &md_content).expect("write md to staging2");

    let result2 = import_from_path(
        &connect_db,
        workspace_path,
        &staging2_dir,
        topic,
        &provenance,
        None,
    )
    .await
    .expect("re-import should succeed");

    assert_eq!(
        result2.references_created, 0,
        "re-import should create 0 new refs"
    );
    assert_eq!(
        result2.references_skipped, 1,
        "re-import should skip the existing ref"
    );

    // --- Cleanup ---
    db::knowledge::delete_references_by_topic(&connect_db, &result.topic_id)
        .await
        .expect("cleanup refs");

    eprintln!("Full PDF pipeline test passed.");
}
