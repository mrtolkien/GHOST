use std::path::Path;
use std::sync::Arc;

use crate::providers::types::{ChatMessage, ChatRequest, ContentBlock, Provider, Role};

use super::DoclingError;

const VISION_PROMPT: &str = "\
Extract ALL text from this document page. Respond in markdown format.

Rules:
- Preserve the reading order and document structure (headings, lists, tables).
- Render tables as markdown tables.
- For images, photos, logos, or diagrams: describe them as \
  [Image: detailed description of what the image shows].
- Do not skip any text, including fine print, footnotes, and labels.
- Do not add any commentary or explanation. Output only the document content.";

/// Render a PDF page to PNG, send to a vision model, return markdown.
pub async fn extract_page_with_vision(
    provider: &Arc<dyn Provider>,
    model: &str,
    workspace: &Path,
    pdf_path: &Path,
    page_no: u32,
) -> Result<String, DoclingError> {
    let (png_path, tmp_guard) = render_page(workspace, pdf_path, page_no).await?;

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![
                ContentBlock::Image {
                    path: png_path.to_string_lossy().to_string(),
                    mime_type: "image/png".to_string(),
                    filename: format!("page_{page_no}.png"),
                },
                ContentBlock::Text {
                    text: VISION_PROMPT.to_string(),
                },
            ],
        }],
        ..Default::default()
    };

    let response = provider
        .chat(request)
        .await
        .map_err(|e| DoclingError::VisionExtraction(e.to_string()))?;

    let text: String = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    // tmp_guard drops here, cleaning up the temp directory and PNG
    drop(tmp_guard);

    if text.is_empty() {
        return Err(DoclingError::VisionExtraction(
            "vision model returned empty response".into(),
        ));
    }

    Ok(text)
}

/// Render a PDF page to PNG via the render_page.py script.
///
/// Returns `(png_path, temp_dir_guard)`. Keep the guard alive until done
/// with the PNG file.
async fn render_page(
    workspace: &Path,
    pdf_path: &Path,
    page_no: u32,
) -> Result<(std::path::PathBuf, tempfile::TempDir), DoclingError> {
    let script = workspace.join("services/docling/render_page.py");
    if !script.exists() {
        return Err(DoclingError::RenderPage(format!(
            "render_page.py not found at {}",
            script.display()
        )));
    }

    let tmp_dir = tempfile::tempdir().map_err(DoclingError::Io)?;
    let output_path = tmp_dir.path().join(format!("page_{page_no}.png"));

    let result = tokio::process::Command::new("uv")
        .arg("run")
        .arg(&script)
        .arg("--path")
        .arg(pdf_path)
        .arg("--page")
        .arg(page_no.to_string())
        .arg("--output")
        .arg(&output_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| DoclingError::RenderPage(format!("failed to spawn uv: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(DoclingError::RenderPage(stderr.to_string()));
    }

    Ok((output_path, tmp_dir))
}
