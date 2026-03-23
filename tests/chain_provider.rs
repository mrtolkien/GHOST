use std::sync::Arc;

use ghost::providers::chain::ChainProvider;
use ghost::providers::{ChatRequest, ContentBlock, Provider, StopReason};

mod common;

fn success_response() -> ghost::providers::ChatResponse {
    common::response(
        vec![ContentBlock::Text {
            text: "ok".to_string(),
        }],
        StopReason::EndTurn,
    )
}

#[tokio::test]
async fn chain_provider_falls_through_on_retryable_error() {
    // First provider has no queued responses → returns InvalidResponse (retryable).
    // Second provider returns success.
    let p1 = common::MockProvider::new(vec![]);
    let p2 = common::MockProvider::new(vec![success_response()]);

    let chain = ChainProvider::new(vec![
        (
            "failing".to_string(),
            Arc::new(p1) as Arc<dyn Provider>,
            "model-a".to_string(),
        ),
        (
            "working".to_string(),
            Arc::new(p2) as Arc<dyn Provider>,
            "model-b".to_string(),
        ),
    ]);

    let request = ChatRequest {
        model: "placeholder".to_string(),
        ..Default::default()
    };

    let result = chain.chat(request).await;
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.content.len(), 1);
    assert!(
        matches!(
            &response.content[0],
            ContentBlock::Text { text } if text == "ok"
        ),
        "expected text content 'ok', got: {:?}",
        response.content
    );
}
