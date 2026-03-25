use std::path::Path;
use std::sync::Arc;

use crate::config::DoclingConfig;
use crate::providers::types::Provider;

use super::convert::{ConvertOptions, DoclingSource};
use super::error::DoclingError;
use super::markdown::generate_markdown;
use super::quality::assess_pages;
use super::vision;

/// Convert a PDF with quality assessment and optional LLM vision fallback.
///
/// After Docling conversion, each page is assessed for quality. Pages flagged
/// as bad (low text, high image coverage) are rendered to PNG and sent to a
/// vision model for re-extraction. Good pages use the Docling markdown output.
///
/// Returns the final stitched markdown string.
#[tracing::instrument(name = "docling convert_hybrid", skip_all)]
pub async fn convert_hybrid(
    config: &DoclingConfig,
    workspace: &Path,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
    vision_provider: Option<&Arc<dyn Provider>>,
    vision_model: Option<&str>,
) -> Result<String, DoclingError> {
    // Extract the PDF path before consuming `source` in convert().
    let pdf_path = match &source {
        DoclingSource::File { path } => Some(path.to_path_buf()),
        DoclingSource::Url { .. } => None,
    };

    let doc = super::convert::convert(config, workspace, source, options).await?;
    let page_qualities = assess_pages(&doc);

    let bad_pages: Vec<u32> = page_qualities
        .iter()
        .filter(|p| !p.is_good)
        .map(|p| p.page_no)
        .collect();

    // If no bad pages, return Docling markdown for everything.
    if bad_pages.is_empty() {
        return Ok(generate_markdown(&doc, None));
    }

    let (Some(provider), Some(model)) = (vision_provider, vision_model) else {
        tracing::warn!(
            bad_pages = ?bad_pages,
            "pages flagged as low quality but no vision provider configured"
        );
        return Ok(generate_markdown(&doc, None));
    };

    tracing::info!(
        bad_pages = ?bad_pages,
        "re-extracting bad pages via vision model"
    );

    let mut page_markdowns = Vec::new();

    for pq in &page_qualities {
        if pq.is_good {
            page_markdowns.push(generate_markdown(&doc, Some(pq.page_no)));
        } else {
            let Some(ref pdf) = pdf_path else {
                // URL sources can't render pages locally -- fall back to
                // Docling markdown.
                page_markdowns.push(generate_markdown(&doc, Some(pq.page_no)));
                continue;
            };
            match vision::extract_page_with_vision(provider, model, workspace, pdf, pq.page_no)
                .await
            {
                Ok(md) => {
                    tracing::info!(
                        page = pq.page_no,
                        chars = md.len(),
                        "vision extraction succeeded"
                    );
                    page_markdowns.push(md);
                }
                Err(e) => {
                    tracing::warn!(
                        page = pq.page_no,
                        error = %e,
                        "vision fallback failed, using Docling output"
                    );
                    page_markdowns.push(generate_markdown(&doc, Some(pq.page_no)));
                }
            }
        }
    }

    Ok(page_markdowns.join("\n\n"))
}
