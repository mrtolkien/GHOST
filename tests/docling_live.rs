mod common;

use std::path::Path;
use std::sync::Arc;

/// Test with MockProvider: Docling converts the lotion PDF, quality check flags
/// the page as bad, vision fallback is invoked with a MockProvider, and the
/// mock response text appears in the final markdown.
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

    // Set up a temp workspace with the render_page.py script in the expected
    // location (services/docling/render_page.py).
    let workspace_dir = tempfile::tempdir().expect("create workspace tempdir");
    let services_dir = workspace_dir.path().join("services/docling");
    std::fs::create_dir_all(&services_dir).expect("create services/docling dir");
    let render_script_src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/services/docling/render_page.py");
    std::fs::copy(&render_script_src, services_dir.join("render_page.py"))
        .expect("copy render_page.py");

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
