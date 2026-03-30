use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::DoclingConfig;
use crate::docling::{self, ConvertOptions, DoclingSource};
use crate::providers::types::Provider;

use super::error::ConvertError;
use super::staging::{create_staging_dir, slug_from_source};

/// Subdirectory within the staging dir for preserving original source files.
const ORIGINALS_SUBDIR: &str = "_originals";

/// Result of converting a PDF file into a staging directory.
#[derive(Debug)]
#[must_use]
pub struct PdfConvertResult {
    /// Path to the staging directory containing the converted markdown.
    pub staging_dir: PathBuf,
    /// The single markdown file within the staging dir (e.g., "report.md").
    pub markdown_file: String,
}

/// Optional vision model for LLM-based fallback on low-quality pages.
///
/// When docling's OCR produces low-quality output for certain pages, the
/// hybrid converter can re-extract those pages using an LLM vision model.
/// Both provider and model name must be supplied together.
pub struct VisionFallback {
    pub provider: Arc<dyn Provider>,
    pub model: String,
}

impl std::fmt::Debug for VisionFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisionFallback")
            .field("provider", &self.provider.name())
            .field("model", &self.model)
            .finish()
    }
}

/// Convert a PDF file to markdown via docling and write to a staging directory.
///
/// Uses [`docling::convert_hybrid`] for conversion with quality assessment and
/// optional LLM vision fallback. The original PDF is preserved in an
/// `_originals/` subdirectory within the staging dir.
#[tracing::instrument(
    name = "convert_pdf",
    skip_all,
    fields(path = %path.display(), staging_root = %staging_root.display())
)]
pub async fn convert_pdf(
    staging_root: &Path,
    path: &Path,
    workspace: &Path,
    docling_config: &DoclingConfig,
    no_ocr: bool,
    page_range: Option<(u32, u32)>,
    vision: Option<VisionFallback>,
) -> Result<PdfConvertResult, ConvertError> {
    // Validate input file exists
    if !path.exists() {
        return Err(ConvertError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", path.display()),
        )));
    }

    let stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("file");
    let original_filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");

    let slug = slug_from_source(&path.to_string_lossy());
    let staging_dir = create_staging_dir(staging_root, &slug)?;

    // Convert via docling with hybrid vision fallback
    let convert_opts = ConvertOptions {
        ocr: !no_ocr,
        page_range,
    };

    let (vision_provider, vision_model) = match &vision {
        Some(v) => (Some(&v.provider), Some(v.model.as_str())),
        None => (None, None),
    };

    let markdown = docling::convert_hybrid(
        docling_config,
        workspace,
        DoclingSource::File { path },
        &convert_opts,
        vision_provider,
        vision_model,
    )
    .await
    .map_err(|e| ConvertError::Conversion(e.to_string()))?;

    if markdown.is_empty() {
        return Err(ConvertError::Conversion(
            "docling produced empty output".into(),
        ));
    }

    // Write markdown to staging
    let markdown_file = format!("{stem}.md");
    let dest = staging_dir.join(&markdown_file);
    std::fs::write(&dest, &markdown)?;

    // Preserve original PDF in _originals/ subdirectory
    let originals_dir = staging_dir.join(ORIGINALS_SUBDIR);
    std::fs::create_dir_all(&originals_dir)?;
    std::fs::copy(path, originals_dir.join(original_filename))?;

    Ok(PdfConvertResult {
        staging_dir,
        markdown_file,
    })
}
