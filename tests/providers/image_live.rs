use std::collections::BTreeMap;

use ghost::providers::{ChatMessage, ChatRequest, ChatResponse, ContentBlock, Provider, Role};

/// Create a 100x100 red square PNG in a temp file and return the path.
fn create_test_image() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path().join("red_square.png");

    let mut img = image::RgbImage::new(100, 100);
    for pixel in img.pixels_mut() {
        *pixel = image::Rgb([255, 0, 0]);
    }
    img.save(&path).expect("save test image");

    (dir, path)
}

/// Build a request with a user message containing text + an image.
fn image_describe_request(model: &str, image_path: &std::path::Path) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![
                ContentBlock::Text {
                    text: "Describe this image in one sentence. What color is it?".to_string(),
                },
                ContentBlock::Image {
                    path: image_path.to_string_lossy().to_string(),
                    mime_type: "image/png".to_string(),
                    filename: "red_square.png".to_string(),
                },
            ],
        }],
        tools: None,
        max_tokens: Some(256),
        temperature: Some(0.0),
        system: Some("You are a helpful assistant. Answer concisely.".to_string()),
        reasoning_effort: None,
        cache_key: "test".to_string(),
        turn_state: None,
        debug_context: None,
            text_verbosity: None,
    }
}

fn assert_describes_red(response: &ChatResponse) {
    let text: String = response
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");

    assert!(
        !text.trim().is_empty(),
        "model returned empty response for image description"
    );

    let lower = text.to_lowercase();
    assert!(
        lower.contains("red"),
        "expected model to mention 'red' in description, got: {text}"
    );
}

// ── OpenAI-compatible (OpenRouter) ──────────────────────────────────────────

#[tokio::test]
async fn openrouter_describes_image() {
    let _obs = ghost::observability::init_for_live_tests().expect("init live test observability");

    if std::env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        eprintln!("OPENROUTER_API_KEY not set; skipping.");
        return;
    }

    let (_dir, image_path) = create_test_image();
    let provider = ghost::providers::OpenRouterProvider::new(BTreeMap::new(), None)
        .expect("OpenRouterProvider init");
    let request = image_describe_request("openai/gpt-4.1-mini", &image_path);

    let response = provider.chat(request).await.expect("provider chat");
    assert_describes_red(&response);
}

// ── Codex Responses API (OpenAI OAuth) ──────────────────────────────────────

#[tokio::test]
async fn codex_responses_describes_image() {
    let _obs = ghost::observability::init_for_live_tests().expect("init live test observability");

    if ghost::auth::openai_oauth::auth_status()
        .await
        .expect("read oauth auth status")
        .is_none()
    {
        eprintln!("No OpenAI OAuth token found; run `ghost auth codex` first. Skipping.");
        return;
    }

    let (_dir, image_path) = create_test_image();
    let provider = ghost::providers::OpenAiOAuthProvider::new(BTreeMap::new())
        .expect("OpenAiOAuthProvider init");
    let request = image_describe_request("gpt-5.3-codex", &image_path);

    let response = provider.chat(request).await.expect("provider chat");
    assert_describes_red(&response);
}
