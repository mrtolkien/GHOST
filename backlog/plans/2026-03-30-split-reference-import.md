# Split Reference Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the monolithic reference import pipeline into two independent stages —
`ghost convert` (source -> markdown staging) and `ghost reference import` (markdown ->
references + DB) — and unify web cache curation to use the same import path.

**Architecture:** New `src/convert/` module owns all source-to-markdown conversion with
no DB dependency. `reference_import::import` gets a single `import_from_path()` entry
point used by the CLI, `reference update`, and web cache curation. The existing
`fetch_*_manifest()` functions move to `convert/` and write to staging dirs instead of
returning in-memory vecs.

**Tech Stack:** Rust, clap (CLI), tokio (async), sqlx (SQLite), docling (PDF
conversion), tempfile (staging), tracing (instrumentation).

**Spec:** `backlog/tasks/5-import-v2/00-split-import.md`

---

## File Structure

### New files

- `src/convert/mod.rs` — barrel: re-exports from submodules
- `src/convert/staging.rs` — staging dir creation, slug generation, provenance stdout
  format
- `src/convert/git.rs` — git clone -> staging dir of markdown files (from
  `reference_import::git::fetch_git_manifest`)
- `src/convert/crawl.rs` — BFS crawl -> staging dir of markdown files (from
  `reference_import::crawl::fetch_crawl_manifest`)
- `src/convert/pdf.rs` — PDF -> staging dir with single markdown file (from
  `reference_import::file::import_file` docling portion)
- `src/cli/convert.rs` — `ghost convert {pdf,git,crawl}` CLI subcommands
- `src/reference_import/import.rs` — generic `import_from_path()` function

### Modified files

- `src/main.rs` — add `Convert` variant to `Commands` enum, remove `Document` variant
- `src/cli/mod.rs` — add `convert` module, remove `document` module
- `src/cli/reference.rs` — rewrite `ReferenceImportCommand` to accept path + topic +
  provenance flags instead of Git/Crawl variants
- `src/reference_import/mod.rs` — update exports (remove per-source imports, add
  `import_from_path`)
- `src/reference_import/types.rs` — remove `ImportSource` enum, add provenance struct
- `src/reference_import/update.rs` — call `convert::git`/`convert::crawl` for re-fetch,
  then diff staging dir
- `src/web/curation.rs` — `curate_references()` and `link_cited_edges()` call
  `import_from_path()` instead of inline file move + DB write
- `assets/skills/reference-import/skill.md` — rewrite for two-step flow
- `assets/skills/document-import/skill.md` — merge into reference-import skill
- `docs/src/content/docs/knowledge/reference-import.md` — update for new CLI

### Removed files

- `src/cli/document.rs` — replaced by `src/cli/convert.rs`
- `src/reference_import/git.rs` — logic moves to `src/convert/git.rs`
- `src/reference_import/crawl.rs` — logic moves to `src/convert/crawl.rs`
- `src/reference_import/file.rs` — logic moves to `src/convert/pdf.rs`

### Test files

- `tests/reference_import_git.rs` — update to use two-step flow (convert then import)
- `tests/reference_import_crawl.rs` — update to use two-step flow
- New unit tests inline in `src/convert/staging.rs` and `src/reference_import/import.rs`

---

## Task 1: Create `src/convert/staging.rs` — staging utilities

**Files:**

- Create: `src/convert/staging.rs`
- Create: `src/convert/mod.rs`

This is the foundation — slug generation and staging dir management that all converters
will use.

- [ ] **Step 1: Write unit tests for slug generation**

Add to `src/convert/staging.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_from_git_url() {
        assert_eq!(
            slug_from_source("https://github.com/DioxusLabs/docsite"),
            "dioxuslabs-docsite"
        );
    }

    #[test]
    fn slug_from_git_url_with_dot_git() {
        assert_eq!(
            slug_from_source("https://github.com/user/repo.git"),
            "user-repo"
        );
    }

    #[test]
    fn slug_from_crawl_url() {
        assert_eq!(
            slug_from_source("https://ghost.tolki.dev/docs/getting-started"),
            "ghost-tolki-dev"
        );
    }

    #[test]
    fn slug_from_file_path() {
        assert_eq!(
            slug_from_source("/home/user/docs/quarterly-report.pdf"),
            "quarterly-report"
        );
    }

    #[test]
    fn slug_deduplicates_in_staging_dir() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");

        let first = create_staging_dir(&staging, "test-slug").unwrap();
        assert_eq!(first.file_name().unwrap(), "test-slug");

        let second = create_staging_dir(&staging, "test-slug").unwrap();
        assert_eq!(second.file_name().unwrap(), "test-slug-2");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib convert::staging -- --nocapture 2>&1 | head -30` Expected:
compilation failure — module doesn't exist yet.

- [ ] **Step 3: Implement staging utilities**

Create `src/convert/mod.rs`:

```rust
pub mod staging;
```

Create `src/convert/staging.rs`:

```rust
use std::path::{Path, PathBuf};

/// Derive a short slug from a source identifier (URL or file path).
///
/// - Git URLs: `owner-repo` (strips `.git` suffix)
/// - HTTP URLs: domain only (e.g., `ghost-tolki-dev`)
/// - File paths: file stem (e.g., `quarterly-report`)
pub fn slug_from_source(source: &str) -> String {
    if source.starts_with("http://") || source.starts_with("https://") {
        if let Ok(url) = url::Url::parse(source) {
            // Git URLs: use last two path segments (owner/repo)
            let path_segments: Vec<&str> = url
                .path_segments()
                .map(|s| s.collect())
                .unwrap_or_default();
            if path_segments.len() >= 2
                && !path_segments.last().unwrap_or(&"").is_empty()
            {
                let owner = path_segments[path_segments.len() - 2];
                let repo = path_segments[path_segments.len() - 1]
                    .trim_end_matches(".git");
                return slugify(&format!("{owner}-{repo}"));
            }
            // Crawl URLs: domain only
            if let Some(host) = url.host_str() {
                return slugify(host);
            }
        }
    }

    // File paths: use the file stem
    Path::new(source)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(slugify)
        .unwrap_or_else(|| "import".to_string())
}

/// Create a staging directory, appending a numeric suffix if it already exists.
pub fn create_staging_dir(staging_root: &Path, slug: &str) -> Result<PathBuf, std::io::Error> {
    std::fs::create_dir_all(staging_root)?;

    let candidate = staging_root.join(slug);
    if !candidate.exists() {
        std::fs::create_dir_all(&candidate)?;
        return Ok(candidate);
    }

    for i in 2..100 {
        let suffixed = staging_root.join(format!("{slug}-{i}"));
        if !suffixed.exists() {
            std::fs::create_dir_all(&suffixed)?;
            return Ok(suffixed);
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("too many staging dirs for slug '{slug}'"),
    ))
}

/// Lowercase, replace non-alphanumeric with hyphens, collapse runs, trim edges.
fn slugify(s: &str) -> String {
    let slug: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse consecutive hyphens and trim
    let mut result = String::with_capacity(slug.len());
    let mut prev_hyphen = true; // trim leading
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push(c);
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    result.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    // ... tests from Step 1 ...
}
```

- [ ] **Step 4: Register the module**

Add `mod convert;` to `src/main.rs` or `src/lib.rs` (wherever modules are declared —
check the existing pattern for `reference_import`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib convert::staging -- --nocapture` Expected: all 5 tests pass.

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: clean pass.

- [ ] **Step 7: Commit**

```bash
git add src/convert/
git commit -m "feat: add convert module with staging utilities"
```

---

## Task 2: Create `src/convert/git.rs` — git clone to staging

**Files:**

- Create: `src/convert/git.rs`
- Modify: `src/convert/mod.rs`

Move `fetch_git_manifest()` logic from `src/reference_import/git.rs` into a function
that writes files to a staging directory instead of returning an in-memory vec.

- [ ] **Step 1: Write `convert::git::convert_git()`**

Create `src/convert/git.rs`. This is adapted from `reference_import::git.rs` lines 13-87
(`fetch_git_manifest`) plus lines 202-290 (helpers). The key change: instead of
returning `Vec<(String, String)>`, it writes files to the staging dir.

```rust
use std::path::{Path, PathBuf};

use crate::convert::staging::{create_staging_dir, slug_from_source};
use crate::reference_import::ImportError;

/// Result of a git conversion: staging dir path + commit hash.
pub struct GitConvertResult {
    pub staging_dir: PathBuf,
    pub version_ref: String,
    pub source_url: String,
}

/// Clone a git repo and write filtered files to a staging directory.
///
/// The staging dir is created under `staging_root` with an auto-generated slug.
/// Returns the staging dir path and the commit hash.
#[tracing::instrument(skip_all, fields(url = url, git_ref))]
pub async fn convert_git(
    staging_root: &Path,
    url: &str,
    paths: &[String],
    extensions: &[String],
    git_ref: Option<&str>,
) -> Result<GitConvertResult, ImportError> {
    let tmp = tempfile::tempdir().map_err(ImportError::Io)?;
    let repo_dir = tmp.path().join("repo");

    // Phase 1: shallow blobless clone
    let mut clone_args = vec![
        "clone",
        "--no-checkout",
        "--depth",
        "1",
        "--filter=blob:none",
    ];
    if let Some(r) = git_ref {
        clone_args.extend(["--branch", r]);
    }
    clone_args.push(url);
    clone_args.push(repo_dir.to_str().unwrap_or("repo"));
    run_git(&clone_args).await?;

    // Phase 2: sparse checkout if paths specified
    if !paths.is_empty() {
        run_git_in(&repo_dir, &["sparse-checkout", "set", "--no-cone"]).await?;
        let patterns: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
        let mut args = vec!["sparse-checkout", "add"];
        args.extend(patterns);
        run_git_in(&repo_dir, &args).await?;
    }
    run_git_in(&repo_dir, &["checkout"]).await?;

    // Get commit hash
    let version_ref = run_git_output(&repo_dir, &["rev-parse", "HEAD"]).await?;

    // Walk files and write to staging
    let slug = slug_from_source(url);
    let staging_dir = create_staging_dir(staging_root, &slug).map_err(ImportError::Io)?;

    let files = walk_files(&repo_dir, paths, extensions);
    if files.is_empty() {
        // Clean up staging dir if nothing found
        let _ = std::fs::remove_dir(&staging_dir);
        return Err(ImportError::Git(format!(
            "no files matched filters in {url}"
        )));
    }

    for file_path in &files {
        let rel = file_path
            .strip_prefix(&repo_dir)
            .unwrap_or(file_path);
        let dest = staging_dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(ImportError::Io)?;
        }
        std::fs::copy(file_path, &dest).map_err(ImportError::Io)?;
    }

    Ok(GitConvertResult {
        staging_dir,
        version_ref,
        source_url: url.to_string(),
    })
}

// --- helpers (moved from reference_import::git) ---

fn walk_files(repo_dir: &Path, paths: &[String], extensions: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if paths.is_empty() {
        walk_dir_recursive(repo_dir, extensions, &mut files);
    } else {
        for p in paths {
            let dir = repo_dir.join(p);
            if dir.is_dir() {
                walk_dir_recursive(&dir, extensions, &mut files);
            } else if dir.is_file() && matches_extensions(&dir, extensions) {
                files.push(dir);
            }
        }
    }
    files
}

fn walk_dir_recursive(dir: &Path, extensions: &[String], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            walk_dir_recursive(&path, extensions, out);
        } else if matches_extensions(&path, extensions) {
            out.push(path);
        }
    }
}

fn matches_extensions(path: &Path, extensions: &[String]) -> bool {
    if extensions.is_empty() {
        return true;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let dotted = format!(".{ext}");
    extensions.iter().any(|e| e.eq_ignore_ascii_case(&dotted))
}

async fn run_git(args: &[&str]) -> Result<(), ImportError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(format!("git {}: {stderr}", args[0])));
    }
    Ok(())
}

async fn run_git_in(dir: &Path, args: &[&str]) -> Result<(), ImportError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(format!("git {}: {stderr}", args[0])));
    }
    Ok(())
}

async fn run_git_output(dir: &Path, args: &[&str]) -> Result<String, ImportError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| ImportError::Git(format!("failed to spawn git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ImportError::Git(format!("git {}: {stderr}", args[0])));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
```

- [ ] **Step 2: Update `src/convert/mod.rs`**

```rust
pub mod git;
pub mod staging;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check 2>&1 | head -20` Expected: clean (or only unrelated warnings).

- [ ] **Step 4: Commit**

```bash
git add src/convert/git.rs src/convert/mod.rs
git commit -m "feat: add convert::git — git clone to staging dir"
```

---

## Task 3: Create `src/convert/crawl.rs` — web crawl to staging

**Files:**

- Create: `src/convert/crawl.rs`
- Modify: `src/convert/mod.rs`

Move `fetch_crawl_manifest()` logic from `src/reference_import/crawl.rs` into a function
that writes files to a staging directory.

- [ ] **Step 1: Write `convert::crawl::convert_crawl()`**

Create `src/convert/crawl.rs`. Adapted from `reference_import::crawl.rs` lines 19-103
(`fetch_crawl_manifest`) plus lines 204-211 (`normalize_url`). Key change: writes
markdown files to staging dir instead of returning in-memory vec.

```rust
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use url::Url;

use crate::convert::staging::{create_staging_dir, slug_from_source};
use crate::reference_import::ImportError;

const CRAWL_DELAY_MS: u64 = 200;

/// Result of a crawl conversion.
pub struct CrawlConvertResult {
    pub staging_dir: PathBuf,
    pub source_url: String,
    /// Map from relative filename to source URL for each crawled page.
    pub page_urls: Vec<(String, String)>,
}

/// BFS-crawl a website and write extracted markdown to a staging directory.
#[tracing::instrument(skip_all, fields(url = url, max_depth, max_pages))]
pub async fn convert_crawl(
    staging_root: &Path,
    url: &str,
    max_depth: usize,
    max_pages: usize,
) -> Result<CrawlConvertResult, ImportError> {
    let seed = Url::parse(url)
        .map_err(|e| ImportError::Fetch(format!("invalid URL '{url}': {e}")))?;
    let seed_host = seed
        .host_str()
        .ok_or_else(|| ImportError::Fetch(format!("URL has no host: {url}")))?
        .to_string();

    let mut queue: VecDeque<(Url, usize)> = VecDeque::new();
    queue.push_back((seed, 0));
    let mut visited: HashSet<String> = HashSet::new();
    let mut pages: Vec<(String, String, String)> = Vec::new(); // (filename, content, source_url)

    while let Some((page_url, depth)) = queue.pop_front() {
        let normalized = normalize_url(&page_url);
        if !visited.insert(normalized.clone()) {
            continue;
        }
        if pages.len() >= max_pages {
            break;
        }

        // Fetch page
        let html = match crate::web::fetch_raw(&page_url.to_string()).await {
            Ok(h) => h,
            Err(e) => {
                return Err(ImportError::Fetch(format!(
                    "failed to fetch {page_url}: {e}"
                )));
            }
        };

        // Extract links if we haven't reached max depth
        if depth < max_depth {
            let links = extract_links(&html, &page_url, &seed_host);
            for link in links {
                let norm = normalize_url(&link);
                if !visited.contains(&norm) {
                    queue.push_back((link, depth + 1));
                }
            }
        }

        // Convert to markdown
        let markdown = crate::web::extract_content(&html, &page_url.to_string());
        if markdown.trim().is_empty() {
            continue;
        }

        let filename = url_to_filename(&page_url);
        pages.push((filename, markdown, page_url.to_string()));

        tokio::time::sleep(std::time::Duration::from_millis(CRAWL_DELAY_MS)).await;
    }

    if pages.is_empty() {
        return Err(ImportError::Fetch(format!(
            "no pages extracted from crawl of {url}"
        )));
    }

    // Write to staging
    let slug = slug_from_source(url);
    let staging_dir = create_staging_dir(staging_root, &slug).map_err(ImportError::Io)?;
    let mut page_urls = Vec::new();

    for (filename, content, source_url) in &pages {
        let dest = staging_dir.join(filename);
        std::fs::write(&dest, content).map_err(ImportError::Io)?;
        page_urls.push((filename.clone(), source_url.clone()));
    }

    Ok(CrawlConvertResult {
        staging_dir,
        source_url: url.to_string(),
        page_urls,
    })
}

fn normalize_url(url: &Url) -> String {
    let mut normalized = url.clone();
    normalized.set_fragment(None);
    normalized.set_query(None);
    normalized
        .as_str()
        .trim_end_matches('/')
        .to_string()
}

fn url_to_filename(url: &Url) -> String {
    let path = url.path().trim_matches('/');
    let slug = if path.is_empty() {
        "index"
    } else {
        path
    };
    let sanitized: String = slug
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    format!("{sanitized}.md")
}

fn extract_links(html: &str, base: &Url, seed_host: &str) -> Vec<Url> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("a[href]").unwrap_or_else(|_| {
        // Fallback: this selector is always valid
        scraper::Selector::parse("a").expect("a selector")
    });

    let mut links = Vec::new();
    for element in document.select(&selector) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        let Ok(resolved) = base.join(href) else {
            continue;
        };
        if resolved.host_str() == Some(seed_host) && resolved.scheme().starts_with("http") {
            links.push(resolved);
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        let url = Url::parse("https://example.com/page/?q=1#frag").unwrap();
        assert_eq!(normalize_url(&url), "https://example.com/page");
    }

    #[test]
    fn test_url_to_filename() {
        let url = Url::parse("https://example.com/docs/getting-started").unwrap();
        assert_eq!(url_to_filename(&url), "docs-getting-started.md");
    }

    #[test]
    fn test_url_to_filename_index() {
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(url_to_filename(&url), "index.md");
    }
}
```

- [ ] **Step 2: Update `src/convert/mod.rs`**

```rust
pub mod crawl;
pub mod git;
pub mod staging;
```

- [ ] **Step 3: Run tests and verify compilation**

Run: `cargo test --lib convert::crawl -- --nocapture` Expected: unit tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/convert/crawl.rs src/convert/mod.rs
git commit -m "feat: add convert::crawl — web crawl to staging dir"
```

---

## Task 4: Create `src/convert/pdf.rs` — PDF to staging

**Files:**

- Create: `src/convert/pdf.rs`
- Modify: `src/convert/mod.rs`

Extract the docling conversion logic from `reference_import::file.rs` (lines 86-110)
into a standalone function that writes markdown to a staging dir.

- [ ] **Step 1: Write `convert::pdf::convert_pdf()`**

Create `src/convert/pdf.rs`:

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::DoclingConfig;
use crate::convert::staging::{create_staging_dir, slug_from_source};
use crate::docling::{self, ConvertOptions, DoclingSource};
use crate::providers::Provider;
use crate::reference_import::ImportError;

/// Result of a PDF conversion.
pub struct PdfConvertResult {
    pub staging_dir: PathBuf,
    /// The single markdown file within the staging dir.
    pub markdown_file: String,
}

/// Convert a PDF to markdown and write to a staging directory.
///
/// The original file is NOT copied — only the converted markdown is written.
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub async fn convert_pdf(
    staging_root: &Path,
    path: &Path,
    docling_config: &DoclingConfig,
    no_ocr: bool,
    page_range: Option<(u32, u32)>,
    vision_provider: Option<Arc<dyn Provider>>,
    vision_model: Option<String>,
) -> Result<PdfConvertResult, ImportError> {
    if !path.exists() {
        return Err(ImportError::Config(format!(
            "file not found: {}",
            path.display()
        )));
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let filename = format!("{stem}.md");

    let source = DoclingSource::File {
        path: path.to_path_buf(),
    };
    let opts = ConvertOptions {
        ocr: !no_ocr,
        page_range,
    };

    let doc = docling::convert_hybrid(
        docling_config,
        source,
        &opts,
        vision_provider,
        vision_model,
    )
    .await?;

    let markdown = crate::docling::markdown::generate_markdown(&doc);
    if markdown.trim().is_empty() {
        return Err(ImportError::Config(
            "docling produced empty output".to_string(),
        ));
    }

    let slug = slug_from_source(&path.display().to_string());
    let staging_dir = create_staging_dir(staging_root, &slug).map_err(ImportError::Io)?;

    let dest = staging_dir.join(&filename);
    std::fs::write(&dest, &markdown).map_err(ImportError::Io)?;

    // Copy original file for preservation
    let originals_dir = staging_dir.join("_originals");
    std::fs::create_dir_all(&originals_dir).map_err(ImportError::Io)?;
    if let Some(original_name) = path.file_name() {
        std::fs::copy(path, originals_dir.join(original_name)).map_err(ImportError::Io)?;
    }

    Ok(PdfConvertResult {
        staging_dir,
        markdown_file: filename,
    })
}
```

- [ ] **Step 2: Update `src/convert/mod.rs`**

```rust
pub mod crawl;
pub mod git;
pub mod pdf;
pub mod staging;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1 | head -20` Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add src/convert/pdf.rs src/convert/mod.rs
git commit -m "feat: add convert::pdf — PDF to staging dir via docling"
```

---

## Task 5: Create `src/reference_import/import.rs` — generic `import_from_path()`

**Files:**

- Create: `src/reference_import/import.rs`
- Modify: `src/reference_import/mod.rs`
- Modify: `src/reference_import/types.rs`

The single entry point for all reference writes: CLI, update, and curation.

- [ ] **Step 1: Add provenance type to `types.rs`**

Add to `src/reference_import/types.rs` (keep existing types that are still used —
`ImportResult`, `UpdateResult`, `ImportError`, `ImportConfigJson`):

```rust
/// Provenance metadata for an import — optional, passed through from convert step.
#[derive(Debug, Clone, Default)]
pub struct ImportProvenance {
    pub source_type: Option<String>,
    pub source_url: Option<String>,
    pub version_ref: Option<String>,
    pub git_ref: Option<String>,
}
```

- [ ] **Step 2: Write the `import_from_path()` function**

Create `src/reference_import/import.rs`:

```rust
use std::path::Path;

use crate::db::{self, GhostDb};
use crate::embeddings::pipeline::content_hash;
use crate::knowledge;
use crate::reference_import::topic::{ensure_topic_hierarchy, write_import_toml};
use crate::reference_import::types::{ImportError, ImportProvenance, ImportResult};

/// Import a file or directory of markdown files into the references system.
///
/// This is the single entry point for all reference creation — used by:
/// - `ghost reference import` CLI command
/// - `reference update` (after converting fresh source)
/// - web cache curation
///
/// If `path` is a file, imports that single file.
/// If `path` is a directory, imports all `.md` files recursively.
///
/// The `source_url` parameter allows per-file source URL association
/// (used by crawl imports and web cache curation).
#[tracing::instrument(skip(db), fields(topic = topic, path = %path.display()))]
pub async fn import_from_path(
    db: &GhostDb,
    workspace: &Path,
    path: &Path,
    topic: &str,
    provenance: &ImportProvenance,
    source_url: Option<&str>,
) -> Result<ImportResult, ImportError> {
    if !path.exists() {
        return Err(ImportError::Config(format!(
            "path not found: {}",
            path.display()
        )));
    }

    let topic_id = ensure_topic_hierarchy(db, topic).await?;

    let files = if path.is_file() {
        vec![(
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file.md")
                .to_string(),
            path.to_path_buf(),
        )]
    } else {
        collect_markdown_files(path)?
    };

    if files.is_empty() {
        return Err(ImportError::Config(format!(
            "no markdown files found in {}",
            path.display()
        )));
    }

    let refs_dir = workspace.join("references").join(topic);
    std::fs::create_dir_all(&refs_dir).map_err(ImportError::Io)?;

    let mut created = 0usize;
    let mut skipped = 0usize;

    // Copy _originals if present in source
    if path.is_dir() {
        let originals_src = path.join("_originals");
        if originals_src.is_dir() {
            let originals_dest = refs_dir.join("_originals");
            std::fs::create_dir_all(&originals_dest).map_err(ImportError::Io)?;
            copy_dir_contents(&originals_src, &originals_dest)?;
        }
    }

    for (rel_path, file_path) in &files {
        // Check idempotency
        let ref_path = format!("{topic}/{rel_path}");
        if let Ok(Some(_)) = db::knowledge::find_reference_by_path(db, &ref_path).await {
            skipped += 1;
            continue;
        }

        let content = std::fs::read_to_string(file_path).map_err(ImportError::Io)?;
        let hash = content_hash(&content);

        // Write file to references/
        let dest = refs_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(ImportError::Io)?;
        }
        std::fs::write(&dest, &content).map_err(ImportError::Io)?;

        db::knowledge::create_reference(
            db,
            &topic_id,
            &ref_path,
            &content,
            source_url,
            None, // import_batch_id set below
            Some(&hash),
        )
        .await
        .map_err(ImportError::Database)?;

        created += 1;
    }

    // Upsert import batch if we have provenance
    let batch_id = if provenance.source_type.is_some() || provenance.source_url.is_some() {
        let config_json = crate::reference_import::ImportConfigJson {
            source_type: provenance
                .source_type
                .clone()
                .unwrap_or_else(|| "local".to_string()),
            source_url: provenance.source_url.clone().unwrap_or_default(),
            git_ref: provenance.git_ref.clone(),
            paths: Vec::new(),
            extensions: Vec::new(),
            max_depth: None,
            max_pages: None,
        };

        let batch_id = db::knowledge::upsert_import_batch(
            db,
            &topic_id,
            &config_json.source_type,
            &config_json.source_url,
            provenance.version_ref.as_deref(),
            (created + skipped) as i64,
            &serde_json::to_string(&config_json).unwrap_or_default(),
        )
        .await
        .map_err(ImportError::Database)?;

        write_import_toml(
            workspace,
            topic,
            &config_json,
            provenance.version_ref.as_deref(),
            created + skipped,
        )?;

        Some(batch_id)
    } else {
        None
    };

    // Create index notes for the topic
    knowledge::ensure_index_notes(workspace, topic)?;

    Ok(ImportResult {
        topic_id,
        batch_id: batch_id.unwrap_or_default(),
        references_created: created,
        references_skipped: skipped,
    })
}

/// Collect all `.md` files in a directory, returning (relative_path, absolute_path).
fn collect_markdown_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, ImportError> {
    let mut files = Vec::new();
    collect_md_recursive(dir, dir, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0)); // deterministic order
    Ok(files)
}

fn collect_md_recursive(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), ImportError> {
    let entries = std::fs::read_dir(dir).map_err(ImportError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip _originals, _orphaned, and hidden dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('_') || name.starts_with('.') {
                continue;
            }
            collect_md_recursive(root, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| ImportError::Config(e.to_string()))?;
            out.push((rel.to_string_lossy().to_string(), path));
        }
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<(), ImportError> {
    for entry in std::fs::read_dir(src).map_err(ImportError::Io)?.flatten() {
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_file() {
            std::fs::copy(&src_path, &dest_path).map_err(ImportError::Io)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Update `src/reference_import/mod.rs`**

Replace the current exports with:

```rust
mod import;
mod topic;
pub mod types;
mod update;

pub use import::import_from_path;
pub use topic::{ensure_topic_hierarchy, load_import_config_from_db, read_import_toml};
pub use types::{
    ImportConfigJson, ImportError, ImportProvenance, ImportResult, UpdateResult,
};
pub use update::update_references;
```

Note: the old `crawl`, `file`, `git` submodules are no longer declared here. Their code
has moved to `src/convert/`. Do NOT delete the old files yet — they are still referenced
by existing code (update.rs, CLI, tests). We will remove them in Task 8 after all
callers are migrated.

**Important:** Keep the old submodule declarations temporarily alongside the new ones
until Task 8. The mod.rs should look like:

```rust
mod crawl;  // keep temporarily — update.rs still uses fetch_crawl_manifest
mod file;   // keep temporarily — not yet removed
mod git;    // keep temporarily — update.rs still uses fetch_git_manifest
mod import;
pub(crate) mod topic;
pub mod types;
mod update;

pub use crawl::import_crawl;
pub use file::import_file;
pub use git::import_git;
pub use import::import_from_path;
pub use topic::{ensure_topic_hierarchy, load_import_config_from_db, read_import_toml};
pub use types::{
    ImportConfig, ImportConfigJson, ImportError, ImportProvenance, ImportResult,
    ImportSource, UpdateResult,
};
pub use update::update_references;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check 2>&1 | head -30` Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/reference_import/import.rs src/reference_import/mod.rs src/reference_import/types.rs
git commit -m "feat: add import_from_path — single entry point for reference writes"
```

---

## Task 6: Create `src/cli/convert.rs` — CLI for convert commands

**Files:**

- Create: `src/cli/convert.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the CLI command definitions and handler**

Create `src/cli/convert.rs`:

```rust
use std::path::PathBuf;

use clap::Subcommand;

use crate::config;
use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum ConvertCommand {
    /// Convert a PDF document to markdown
    Pdf {
        /// Path to the PDF file
        path: PathBuf,

        /// Disable OCR for scanned pages
        #[arg(long)]
        no_ocr: bool,

        /// Page range to convert (e.g., "1-10")
        #[arg(long)]
        page_range: Option<String>,

        /// Conversion timeout in seconds
        #[arg(long, default_value = "300")]
        timeout: u64,

        /// Output directory (default: workspace/staging/<slug>/)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Clone a git repository and extract files as markdown
    Git {
        /// Git repository URL
        url: String,

        /// Subdirectory paths to include (sparse checkout)
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,

        /// File extensions to include (e.g., .md,.rst)
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,

        /// Branch, tag, or commit to checkout
        #[arg(long, alias = "ref")]
        git_ref: Option<String>,
    },

    /// Crawl a website and convert pages to markdown
    Crawl {
        /// Starting URL to crawl
        url: String,

        /// Maximum crawl depth from seed URL
        #[arg(long, default_value = "3")]
        max_depth: usize,

        /// Maximum number of pages to crawl
        #[arg(long, default_value = "50")]
        max_pages: usize,
    },
}

#[tracing::instrument(skip_all, name = "cli::convert")]
pub async fn execute(command: ConvertCommand) -> Result<(), GhostError> {
    let config = config::load()?;
    let workspace = PathBuf::from(&config.workspace);
    let staging_root = workspace.join("staging");

    match command {
        ConvertCommand::Pdf {
            path,
            no_ocr,
            page_range,
            timeout: _timeout,
            output,
        } => {
            let page_range = parse_page_range(page_range.as_deref())?;
            let (vision_provider, vision_model) =
                crate::providers::resolve_vision_provider(&config).await;

            let staging = output.unwrap_or(staging_root);
            let result = crate::convert::pdf::convert_pdf(
                &staging,
                &path,
                &config.docling,
                no_ocr,
                page_range,
                vision_provider,
                vision_model,
            )
            .await?;

            // Print provenance to stdout for GHOST consumption
            println!("{}", result.staging_dir.display());
            println!("source_type=file");
        }

        ConvertCommand::Git {
            url,
            paths,
            extensions,
            git_ref,
        } => {
            let result = crate::convert::git::convert_git(
                &staging_root,
                &url,
                &paths,
                &extensions,
                git_ref.as_deref(),
            )
            .await?;

            println!("{}", result.staging_dir.display());
            println!("source_type=git");
            println!("source_url={}", result.source_url);
            println!("version_ref={}", result.version_ref);
            if let Some(r) = git_ref {
                println!("git_ref={r}");
            }
        }

        ConvertCommand::Crawl {
            url,
            max_depth,
            max_pages,
        } => {
            let result = crate::convert::crawl::convert_crawl(
                &staging_root,
                &url,
                max_depth,
                max_pages,
            )
            .await?;

            println!("{}", result.staging_dir.display());
            println!("source_type=crawl");
            println!("source_url={}", result.source_url);
        }
    }

    Ok(())
}

fn parse_page_range(s: Option<&str>) -> Result<Option<(u32, u32)>, GhostError> {
    let Some(s) = s else { return Ok(None) };
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return Err(GhostError::Config(format!(
            "invalid page range '{s}', expected format like '1-10'"
        )));
    }
    let start: u32 = parts[0]
        .trim()
        .parse()
        .map_err(|_| GhostError::Config(format!("invalid start page in '{s}'")))?;
    let end: u32 = parts[1]
        .trim()
        .parse()
        .map_err(|_| GhostError::Config(format!("invalid end page in '{s}'")))?;
    Ok(Some((start, end)))
}
```

- [ ] **Step 2: Register in `src/cli/mod.rs`**

Add `pub mod convert;` to the module declarations.

- [ ] **Step 3: Add `Convert` variant to `Commands` enum in `src/main.rs`**

Add alongside the existing `Document` variant (don't remove `Document` yet):

```rust
/// Convert sources to markdown for inspection before import
#[command(subcommand)]
Convert(cli::convert::ConvertCommand),
```

And in the `dispatch()` match arm:

```rust
Commands::Convert(cmd) => cli::convert::execute(cmd).await,
```

- [ ] **Step 4: Verify compilation and help text**

Run: `cargo build 2>&1 | tail -5` Run: `cargo run -- convert --help` Expected: shows
Pdf, Git, Crawl subcommands.

- [ ] **Step 5: Commit**

```bash
git add src/cli/convert.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add ghost convert CLI — pdf, git, crawl subcommands"
```

---

## Task 7: Rewrite `src/cli/reference.rs` — new import command

**Files:**

- Modify: `src/cli/reference.rs`

Replace the `ReferenceImportCommand` enum (Git/Crawl variants) with a single import
command that accepts path + topic + provenance flags.

- [ ] **Step 1: Rewrite the import command**

Replace `ReferenceImportCommand` enum and its handler in `src/cli/reference.rs`. Keep
`Update` and `Delete` commands unchanged.

The new `Import` variant:

```rust
#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    /// Import markdown files or directories as references
    Import {
        /// Path to a markdown file or directory of markdown files
        path: PathBuf,

        /// Topic name (hierarchical, e.g., "dioxus/docs")
        #[arg(long)]
        topic: String,

        /// Source type (git, crawl, file)
        #[arg(long)]
        source_type: Option<String>,

        /// Source URL
        #[arg(long)]
        source_url: Option<String>,

        /// Version reference (e.g., git commit hash)
        #[arg(long)]
        version_ref: Option<String>,

        /// Git ref (branch or tag)
        #[arg(long)]
        git_ref: Option<String>,
    },

    /// Update references from their original source
    Update {
        // ... keep existing fields
    },

    /// Delete a topic and all its references
    Delete {
        // ... keep existing fields
    },
}
```

The execute handler for `Import`:

```rust
ReferenceCommand::Import {
    path,
    topic,
    source_type,
    source_url,
    version_ref,
    git_ref,
} => {
    let config = config::load()?;
    let db = db::connect(&config.workspace, config.embeddings.dimension).await?;
    let workspace = std::path::Path::new(&config.workspace);

    let provenance = ImportProvenance {
        source_type,
        source_url,
        version_ref,
        git_ref,
    };

    let result = crate::reference_import::import_from_path(
        &db,
        workspace,
        &path,
        &topic,
        &provenance,
        None, // source_url per file — not used in CLI mode
    )
    .await?;

    print_result(&topic, &result);

    // Clean up staging dir if it's under workspace/staging/
    let staging_root = workspace.join("staging");
    if path.starts_with(&staging_root) {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else if let Some(parent) = path.parent() {
            if parent.starts_with(&staging_root) && parent != staging_root {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Update `print_result` to work with new signature**

```rust
fn print_result(topic: &str, result: &ImportResult) {
    println!("References saved to: references/{topic}/");
    println!(
        "  created: {}, skipped: {}",
        result.references_created, result.references_skipped
    );
    println!("Embeddings being computed in background by file watcher");
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1 | head -20` Run: `cargo run -- reference import --help` Expected:
shows path, --topic, --source-type, --source-url, --version-ref, --git-ref.

- [ ] **Step 4: Commit**

```bash
git add src/cli/reference.rs
git commit -m "feat: rewrite reference import CLI — path + topic + provenance flags"
```

---

## Task 8: Migrate `reference update` to use `convert/`

**Files:**

- Modify: `src/reference_import/update.rs`

The `fetch_manifest()` helper in update.rs currently calls `fetch_git_manifest()` and
`fetch_crawl_manifest()` from the old modules. Migrate to call `convert::git` and
`convert::crawl` instead, then diff the staging dir against existing references.

- [ ] **Step 1: Rewrite `fetch_manifest()` in update.rs**

The key change: instead of returning in-memory vecs, the function produces a staging dir
and we read files from it.

Replace the `fetch_manifest()` function (lines 175-199) and update `update_references()`
to work with staging dirs.

```rust
use crate::convert;
use std::collections::HashMap;

/// Re-fetch source into a staging dir, return (version_ref, manifest, staging_dir).
/// The caller must clean up the staging dir.
async fn fetch_to_staging(
    workspace: &Path,
    config: &ImportConfig,
) -> Result<(Option<String>, Vec<(String, String, Option<String>)>, PathBuf), ImportError> {
    let staging_root = tempfile::tempdir().map_err(ImportError::Io)?;
    let staging_path = staging_root.into_path(); // don't auto-cleanup

    match &config.source {
        ImportSource::Git {
            url,
            paths,
            extensions,
            git_ref,
        } => {
            let result = convert::git::convert_git(
                &staging_path,
                url,
                paths,
                extensions,
                git_ref.as_deref(),
            )
            .await?;

            let manifest = read_staging_as_manifest(&result.staging_dir, None)?;
            Ok((Some(result.version_ref), manifest, result.staging_dir))
        }
        ImportSource::Crawl {
            url,
            max_depth,
            max_pages,
        } => {
            let result = convert::crawl::convert_crawl(
                &staging_path,
                url,
                *max_depth,
                *max_pages,
            )
            .await?;

            let url_map: HashMap<String, String> =
                result.page_urls.into_iter().collect();
            let manifest = read_staging_as_manifest(&result.staging_dir, Some(&url_map))?;
            Ok((None, manifest, result.staging_dir))
        }
        ImportSource::File { .. } => Err(ImportError::Config(
            "cannot update file-based imports".to_string(),
        )),
    }
}

/// Read a staging directory into a manifest vec compatible with the diff logic.
fn read_staging_as_manifest(
    staging_dir: &Path,
    url_map: Option<&HashMap<String, String>>,
) -> Result<Vec<(String, String, Option<String>)>, ImportError> {
    let mut manifest = Vec::new();
    read_staging_recursive(staging_dir, staging_dir, url_map, &mut manifest)?;
    Ok(manifest)
}

fn read_staging_recursive(
    root: &Path,
    dir: &Path,
    url_map: Option<&HashMap<String, String>>,
    out: &mut Vec<(String, String, Option<String>)>,
) -> Result<(), ImportError> {
    for entry in std::fs::read_dir(dir).map_err(ImportError::Io)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('_') || name.starts_with('.') {
                continue;
            }
            read_staging_recursive(root, &path, url_map, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| ImportError::Config(e.to_string()))?
                .to_string_lossy()
                .to_string();
            let content = std::fs::read_to_string(&path).map_err(ImportError::Io)?;
            let source_url = url_map.and_then(|m| m.get(&rel).cloned());
            out.push((rel, content, source_url));
        }
    }
    Ok(())
}
```

Then update `update_references()` to use `fetch_to_staging()` instead of
`fetch_manifest()`, and add cleanup of the staging dir at the end (both success and
error paths, using a scope guard or explicit cleanup).

- [ ] **Step 2: Verify compilation**

Run: `cargo check 2>&1 | head -20` Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add src/reference_import/update.rs
git commit -m "refactor: reference update uses convert/ for re-fetch"
```

---

## Task 9: Remove old import modules and `cli/document.rs`

**Files:**

- Delete: `src/reference_import/git.rs`
- Delete: `src/reference_import/crawl.rs`
- Delete: `src/reference_import/file.rs`
- Delete: `src/cli/document.rs`
- Modify: `src/reference_import/mod.rs` — remove old module declarations and re-exports
- Modify: `src/reference_import/types.rs` — remove `ImportSource` and `ImportConfig` if
  no longer used
- Modify: `src/cli/mod.rs` — remove `document` module
- Modify: `src/main.rs` — remove `Document` variant from `Commands`

- [ ] **Step 1: Remove old module files**

Delete the four files listed above.

- [ ] **Step 2: Clean up `reference_import/mod.rs`**

Remove the `mod crawl`, `mod file`, `mod git` declarations and their re-exports
(`import_crawl`, `import_file`, `import_git`, `ImportSource`, `ImportConfig`).

Final `mod.rs`:

```rust
mod import;
pub(crate) mod topic;
pub mod types;
mod update;

pub use import::import_from_path;
pub use topic::{ensure_topic_hierarchy, load_import_config_from_db, read_import_toml};
pub use types::{ImportConfigJson, ImportError, ImportProvenance, ImportResult, UpdateResult};
pub use update::update_references;
```

- [ ] **Step 3: Clean up `types.rs`**

Remove `ImportSource` enum and `ImportConfig` struct if nothing references them anymore.
Keep `ImportConfigJson` (used by update and \_import.toml) and the `From` impl that
`update.rs` still needs. Remove the `to_import_config()` method on `ImportConfigJson` if
`ImportConfig` is gone — update.rs should reconstruct what it needs from
`ImportConfigJson` directly.

Check: `cargo check` will tell you if anything still references the removed types.

- [ ] **Step 4: Remove `cli/document.rs` and `Document` from main**

In `src/cli/mod.rs`: remove `pub mod document;` In `src/main.rs`: remove the `Document`
variant and its match arm in `dispatch()`.

- [ ] **Step 5: Verify everything compiles**

Run: `cargo check 2>&1 | head -30` Fix any remaining references to deleted
types/functions.

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: clean pass (tests may need updating — see Task 10).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: remove old per-source import modules and cli/document"
```

---

## Task 10: Update integration tests

**Files:**

- Modify: `tests/reference_import_git.rs`
- Modify: `tests/reference_import_crawl.rs`

Update the live tests to use the two-step flow: convert to staging, then import from
path.

- [ ] **Step 1: Update git import test**

In `tests/reference_import_git.rs`, replace the direct `import_git()` call with:

```rust
use ghost::convert::git::convert_git;
use ghost::reference_import::{import_from_path, ImportProvenance};

// Phase 1: Convert to staging
let staging_root = workspace_path.join("staging");
let convert_result = convert_git(
    &staging_root,
    "https://github.com/DioxusLabs/docsite",
    &["docs-src/0.7/src/tutorial/".to_string()],
    &[".md".to_string()],
    None,
)
.await
.expect("convert git");

// Phase 2: Import from staging
let provenance = ImportProvenance {
    source_type: Some("git".to_string()),
    source_url: Some("https://github.com/DioxusLabs/docsite".to_string()),
    version_ref: Some(convert_result.version_ref.clone()),
    git_ref: None,
};

let result = import_from_path(
    &db,
    workspace_path,
    &convert_result.staging_dir,
    "dioxus/docs",
    &provenance,
    None,
)
.await
.expect("import from path");
```

Keep all the assertion phases (BM25 search, embeddings, vector search, idempotent
re-import) but update the re-import to also use the two-step flow.

- [ ] **Step 2: Update crawl import test**

Similarly in `tests/reference_import_crawl.rs`, replace `import_crawl()` with
`convert_crawl()` + `import_from_path()`.

- [ ] **Step 3: Run the live tests**

Run: `cargo test --features live-tests -- import --nocapture 2>&1 | tail -30` Expected:
all tests pass.

- [ ] **Step 4: Commit**

```bash
git add tests/reference_import_git.rs tests/reference_import_crawl.rs
git commit -m "test: update import tests for two-step convert + import flow"
```

---

## Task 11: Unify web cache curation with `import_from_path()`

**Files:**

- Modify: `src/web/curation.rs`

Replace the inline file-move + DB-write logic in `curate_references()` and
`link_cited_edges()` with calls to `import_from_path()`.

- [ ] **Step 1: Make `curate_references()` async and use `import_from_path()`**

The function signature changes from sync to async (it now does DB writes via
`import_from_path`):

```rust
pub async fn curate_references(
    db: &GhostDb,
    workspace: &Path,
    session_id: &str,
    classified: &[ClassifiedCacheFile],
) -> CurationResult {
```

Replace the `move_to_references()` call (lines 409-425) with:

```rust
if used && !file.url.is_empty() {
    let domain = topic_from_url(&file.url);
    let topic = match note_topic {
        Some(t) => format!("{t}/{domain}"),
        None => domain,
    };

    let cache_path = cache_dir.join(&file.filename);
    let provenance = ImportProvenance {
        source_type: Some("web".to_string()),
        source_url: Some(file.url.clone()),
        ..Default::default()
    };

    match crate::reference_import::import_from_path(
        db,
        workspace,
        &cache_path,
        &topic,
        &provenance,
        Some(&file.url),
    )
    .await
    {
        Ok(r) => {
            tracing::info!(
                filename = file.filename.clone(),
                topic = topic,
                created = r.references_created,
                "curate_references: imported",
            );
            result.moved += 1;
            // Delete the cache file after successful import
            let _ = std::fs::remove_file(&cache_path);
        }
        Err(e) => {
            tracing::warn!(
                filename = file.filename.clone(),
                error = e.to_string(),
                "curate_references: import failed",
            );
        }
    }
}
```

- [ ] **Step 2: Simplify `link_cited_edges()`**

The DB record creation block in `link_cited_edges()` (lines 226-274) can be simplified.
Since `curate_references()` now creates DB records via `import_from_path()`, the
`link_cited_edges()` function should find existing records rather than creating them.
The fallback creation logic (lines 230-273) can be reduced to just a warning if the
record doesn't exist:

```rust
Ok(None) => {
    // Try by path
    match db::knowledge::find_reference_by_path(db, &rel_path).await {
        Ok(Some(r)) => r,
        _ => {
            tracing::debug!(
                url = file.url.clone(),
                "link_cited_edges: no reference record found, skipping",
            );
            continue;
        }
    }
}
```

- [ ] **Step 3: Update all callers of `curate_references()`**

The function is now async and takes a `db` parameter. Find all call sites (likely in
`src/scripting/bindings.rs`) and update them:

```rust
// Before:
let curation = crate::web::curate_references(&workspace, session_id, &classified);

// After:
let curation = crate::web::curate_references(&db, &workspace, session_id, &classified).await;
```

- [ ] **Step 4: Remove `move_to_references()` function**

Delete the `move_to_references()` function (lines 521-543) — it's no longer called.

- [ ] **Step 5: Verify compilation**

Run: `cargo check 2>&1 | head -30` Expected: clean.

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: clean pass.

- [ ] **Step 7: Commit**

```bash
git add src/web/curation.rs src/scripting/bindings.rs
git commit -m "refactor: web cache curation uses import_from_path for reference writes"
```

---

## Task 12: Update GHOST skills

**Files:**

- Modify: `assets/skills/reference-import/skill.md`
- Delete or redirect: `assets/skills/document-import/skill.md`

- [ ] **Step 1: Rewrite `reference-import` skill**

Rewrite `assets/skills/reference-import/skill.md` to document the two-step flow. Key
sections:

- Decision flow: same 5-step prioritization
- Convert step: `ghost convert {git,crawl,pdf}` — produces staging dir, print output
- Inspect step: GHOST reads staging dir content, decides topic
- Import step: `ghost reference import <path> --topic <topic> --source-type ... `
- Update and delete: unchanged
- Background mode: `run_shell_command` with `background: true` on the convert step, then
  import after reading the result

Keep the same structure as the existing skill but replace all CLI examples with the new
commands.

- [ ] **Step 2: Merge `document-import` into `reference-import`**

The document-import skill's content (PDF download + convert + import) should be folded
into the reference-import skill as a "PDF/Document" section. The key differences:

- Download step (curl) stays
- Convert: `ghost convert pdf <path> [--no-ocr] [--page-range ...]`
- Import: `ghost reference import <staging-dir> --topic <topic> --source-type file`

Then either delete `assets/skills/document-import/skill.md` or replace its content with
a redirect/pointer to the reference-import skill.

- [ ] **Step 3: Verify skills are valid markdown**

Run: `just fmt` to check formatting.

- [ ] **Step 4: Commit**

```bash
git add assets/skills/
git commit -m "docs: update GHOST skills for two-step convert + import flow"
```

---

## Task 13: Update user-facing documentation

**Files:**

- Modify: `docs/src/content/docs/knowledge/reference-import.md`

- [ ] **Step 1: Rewrite the reference import docs**

Read the `/docs` skill first for doc conventions. Update the page to cover:

1. **Overview**: Two-step flow — convert sources to markdown, then import
2. **Convert commands**: `ghost convert pdf`, `ghost convert git`, `ghost convert crawl`
   with flag tables
3. **Import command**: `ghost reference import <path> --topic <topic>` with flag table
4. **Staging directory**: what it is, where it lives, auto-cleanup
5. **Storage model**: unchanged (disk + DB + FTS5 + embeddings)
6. **Topic hierarchy**: unchanged
7. **Updating references**: unchanged command, new internals
8. **Cleanup**: unchanged
9. **How GHOST uses these**: updated to describe two-step skill flow

- [ ] **Step 2: Run doc build**

Run: `just doc` Expected: builds cleanly.

- [ ] **Step 3: Commit**

```bash
git add docs/
git commit -m "docs: update reference import docs for convert + import split"
```

---

## Task 14: Final cleanup and `just ci`

**Files:** Various — any remaining compilation errors or warnings.

- [ ] **Step 1: Run full CI**

Run: `just ci` Fix any clippy warnings, format issues, or test failures.

- [ ] **Step 2: Verify the staging directory is gitignored**

Check that `staging/` is in `.gitignore` (workspace-level). If not, add it:

```
staging/
```

This is the workspace `.gitignore`, not the project `.gitignore`. Check where Ghost
creates the workspace gitignore (likely in `config_workspace.rs` bootstrap).

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "chore: final cleanup for convert + import split"
```
