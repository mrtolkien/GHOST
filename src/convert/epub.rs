use std::path::{Path, PathBuf};

use rbook::Epub;
use serde::{Deserialize, Serialize};

use super::error::ConvertError;
use super::staging::{create_staging_dir, slug_from_source};

/// Subdirectory within the staging dir for preserving original source files.
const ORIGINALS_SUBDIR: &str = "_originals";

/// Metadata file written to staging dir for downstream import.
const METADATA_FILE: &str = "_metadata.json";

/// Minimum content length (bytes, after trim) to keep a spine item.
/// Shorter items are title pages, blank separators, etc.
const MIN_CONTENT_BYTES: usize = 50;

/// Result of converting an EPUB file into a staging directory.
#[derive(Debug)]
#[must_use]
pub struct EpubConvertResult {
    /// Path to the staging directory containing per-chapter markdown files.
    pub staging_dir: PathBuf,
    /// Number of chapter files written (excludes trivial spine items).
    pub chapter_count: usize,
    /// Extracted book metadata.
    pub metadata: EpubMetadata,
}

/// Metadata extracted from the EPUB's OPF package document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
}

/// Convert an EPUB file to per-chapter markdown files in a staging directory.
///
/// Each spine item (reading-order entry) becomes a separate markdown file.
/// Trivial items (< [`MIN_CONTENT_BYTES`] after conversion) are skipped.
/// The original EPUB is preserved in `_originals/`.
#[tracing::instrument(
    name = "convert_epub",
    skip_all,
    fields(path = %epub_path.display())
)]
pub fn convert_epub(
    staging_root: &Path,
    epub_path: &Path,
) -> Result<EpubConvertResult, ConvertError> {
    if !epub_path.exists() {
        return Err(ConvertError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", epub_path.display()),
        )));
    }

    let epub = Epub::open(epub_path)
        .map_err(|e| ConvertError::Conversion(format!("failed to open EPUB: {e}")))?;

    let metadata = extract_metadata(&epub);

    let slug = slug_from_source(&epub_path.to_string_lossy());
    let staging_dir = create_staging_dir(staging_root, &slug)?;

    let converter = build_html_converter();

    let chapter_count = write_chapters(&epub, &converter, &staging_dir, &metadata)?;

    if chapter_count == 0 {
        return Err(ConvertError::Conversion(
            "EPUB produced no chapter content".into(),
        ));
    }

    preserve_original(epub_path, &staging_dir)?;
    write_metadata(&metadata, &staging_dir)?;

    Ok(EpubConvertResult {
        staging_dir,
        chapter_count,
        metadata,
    })
}

/// Build HTML-to-markdown converter (same config as web/fetch.rs).
fn build_html_converter() -> htmd::HtmlToMarkdown {
    htmd::HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "nav", "footer", "header", "noscript", "svg", "iframe",
        ])
        .build()
}

/// Iterate spine items and write non-trivial chapters as markdown files.
fn write_chapters(
    epub: &Epub,
    converter: &htmd::HtmlToMarkdown,
    staging_dir: &Path,
    metadata: &EpubMetadata,
) -> Result<usize, ConvertError> {
    let mut reader = epub.reader();
    let mut chapter_idx: usize = 0;
    let mut chapter_count: usize = 0;
    let title_lower = metadata.title.as_deref().unwrap_or("").to_lowercase();

    while let Some(content_result) = reader.read_next() {
        let data = content_result
            .map_err(|e| ConvertError::Conversion(format!("failed to read spine item: {e}")))?;

        let xhtml = data.content();
        let markdown = converter
            .convert(xhtml)
            .unwrap_or_else(|_| xhtml.to_string());

        let markdown = strip_title_line(&markdown, &title_lower);
        let trimmed = markdown.trim();

        if trimmed.len() < MIN_CONTENT_BYTES {
            chapter_idx += 1;
            continue;
        }

        let href = data.manifest_entry().href();
        let filename = chapter_filename(chapter_idx, href.as_ref());

        std::fs::write(staging_dir.join(&filename), trimmed)?;
        chapter_count += 1;
        chapter_idx += 1;
    }

    Ok(chapter_count)
}

/// Copy original EPUB to `_originals/` subdirectory.
fn preserve_original(epub_path: &Path, staging_dir: &Path) -> Result<(), ConvertError> {
    let originals_dir = staging_dir.join(ORIGINALS_SUBDIR);
    std::fs::create_dir_all(&originals_dir)?;
    if let Some(filename) = epub_path.file_name() {
        std::fs::copy(epub_path, originals_dir.join(filename))?;
    }
    Ok(())
}

/// Write metadata JSON for downstream import.
fn write_metadata(metadata: &EpubMetadata, staging_dir: &Path) -> Result<(), ConvertError> {
    let metadata_json = serde_json::to_string_pretty(metadata)
        .map_err(|e| ConvertError::Conversion(format!("failed to serialize metadata: {e}")))?;
    std::fs::write(staging_dir.join(METADATA_FILE), metadata_json)?;
    Ok(())
}

/// Extract metadata from the EPUB's OPF package document.
///
/// All fields are best-effort -- missing data produces `None` / empty vec.
fn extract_metadata(epub: &Epub) -> EpubMetadata {
    let meta = epub.metadata();

    let title = meta.title().map(|t| t.value().to_string());

    let authors: Vec<String> = meta.creators().map(|c| c.value().to_string()).collect();

    let language = meta.languages().next().map(|l| l.value().to_string());

    let publisher = meta.publishers().next().map(|p| p.value().to_string());

    let publication_date = meta.published().map(|d| d.to_string());

    EpubMetadata {
        title,
        authors,
        language,
        publisher,
        publication_date,
    }
}

/// Strip the book title if it appears as the first non-empty line.
///
/// htmd often includes the `<title>` tag content as the first line of each
/// chapter, producing duplicated title text.
fn strip_title_line<'a>(markdown: &'a str, title_lower: &str) -> &'a str {
    if title_lower.is_empty() {
        return markdown;
    }

    // htmd may emit the book title twice: once from <title> (plain text) and
    // once from <h1> (markdown heading).  Strip both occurrences by applying
    // the check up to two times.
    let mut result = markdown;
    for _ in 0..2 {
        let trimmed = result.trim_start();
        let first_line_end = trimmed.find('\n').unwrap_or(trimmed.len());
        let first_line = trimmed[..first_line_end].trim();

        // Strip markdown heading prefix if present
        let bare = first_line.trim_start_matches('#').trim();

        if bare.to_lowercase() == title_lower {
            result = trimmed[first_line_end..].trim_start_matches('\n');
        } else {
            break;
        }
    }
    result
}

/// Generate a chapter filename from the spine item index and EPUB href.
///
/// Produces `{idx:02}-{sanitized_stem}.md` or `{idx:02}-chapter-{idx}.md`
/// as fallback.
fn chapter_filename(idx: usize, href: &str) -> String {
    let stem = Path::new(href)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let name = if sanitized.is_empty() || sanitized.chars().all(|c| c == '-') {
        format!("chapter-{idx}")
    } else {
        sanitized.trim_matches('-').to_lowercase()
    };

    format!("{idx:02}-{name}.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_title_line_removes_matching_title() {
        let md = "Animal Farm\n\nChapter content here.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, "Chapter content here.");
    }

    #[test]
    fn strip_title_line_removes_heading_title() {
        let md = "# Animal Farm\n\nChapter content here.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, "Chapter content here.");
    }

    #[test]
    fn strip_title_line_preserves_non_matching() {
        let md = "Chapter I\n\nContent here.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, md);
    }

    #[test]
    fn strip_title_line_empty_title() {
        let md = "Whatever\n\nContent.";
        let result = strip_title_line(md, "");
        assert_eq!(result, md);
    }

    #[test]
    fn strip_title_line_removes_double_title() {
        // htmd emits <title> as plain text + <h1> as heading — both need stripping
        let md = "Animal Farm\n\n# Animal Farm\n\n## I\n\nChapter content.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, "## I\n\nChapter content.");
    }

    #[test]
    fn chapter_filename_from_href() {
        assert_eq!(chapter_filename(0, "OEBPS/title.xhtml"), "00-title.md");
        assert_eq!(chapter_filename(2, "OEBPS/part1.xhtml"), "02-part1.md");
        assert_eq!(chapter_filename(10, "chapter-x.html"), "10-chapter-x.md");
    }

    #[test]
    fn chapter_filename_fallback() {
        assert_eq!(chapter_filename(3, ""), "03-chapter-3.md");
        assert_eq!(chapter_filename(5, "///"), "05-chapter-5.md");
    }
}
