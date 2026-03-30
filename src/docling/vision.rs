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

/// Render DPI for PDF page screenshots sent to vision models.
const RENDER_DPI: u32 = 300;

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
        system: Some(VISION_PROMPT.to_string()),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![
                ContentBlock::Image {
                    path: png_path.to_string_lossy().to_string(),
                    mime_type: "image/png".to_string(),
                    filename: format!("page_{page_no}.png"),
                },
                ContentBlock::Text {
                    text: "Extract all text from this document page.".to_string(),
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

/// Render a PDF page to PNG via `pdftoppm` (poppler-utils, fetched on
/// demand through `nix run` — no permanent shell flake dependency).
///
/// Returns `(png_path, temp_dir_guard)`. Keep the guard alive until done
/// with the PNG file.
async fn render_page(
    _workspace: &Path,
    pdf_path: &Path,
    page_no: u32,
) -> Result<(std::path::PathBuf, tempfile::TempDir), DoclingError> {
    let tmp_dir = tempfile::tempdir().map_err(DoclingError::Io)?;
    // pdftoppm appends ".png" to the output prefix with -singlefile
    let output_prefix = tmp_dir.path().join(format!("page_{page_no}"));
    let output_path = tmp_dir.path().join(format!("page_{page_no}.png"));

    let page_str = page_no.to_string();
    let dpi_str = RENDER_DPI.to_string();

    let mut cmd = tokio::process::Command::new("nix");
    cmd.args(["run", "nixpkgs#poppler_utils", "--", "pdftoppm"])
        .args(["-png", "-singlefile"])
        .args(["-f", &page_str, "-l", &page_str])
        .args(["-r", &dpi_str])
        .arg(pdf_path)
        .arg(&output_prefix)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let result = cmd
        .output()
        .await
        .map_err(|e| DoclingError::RenderPage(format!("failed to spawn pdftoppm: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(DoclingError::RenderPage(format!(
            "pdftoppm failed: {stderr}"
        )));
    }

    if !output_path.exists() {
        return Err(DoclingError::RenderPage(format!(
            "pdftoppm produced no output at {}",
            output_path.display()
        )));
    }

    Ok((output_path, tmp_dir))
}
