# Docling Improvements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement
> this plan task-by-task.

**Goal:** Switch docling client to async API (6x faster), add operator-facing options
(no-ocr, page-range, timeout), split CLI into `ghost reference import git|crawl` and
`ghost document import url|file`.

**Architecture:** Unified async docling client replaces two sync endpoints. New
`[docling]` config section replaces `[web].docling_url`. CLI split into two command
groups with separate skills. All changes flow through `src/web/docling.rs` →
`src/reference_import/{page,file}.rs` → `src/cli/{reference,document}.rs`.

**Tech Stack:** Rust, reqwest, serde_json, clap, tokio, base64

**Spec:** `specs/2_docling-improvements.md`

---

### Task 1: Config — Add `[docling]` section

**Files:**
- Modify: `src/config.rs`

**Step 1: Add DoclingSettings and DoclingConfig structs**

Add after `WebSettings` (around line 150):

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DoclingSettings {
    pub url: Option<String>,
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoclingConfig {
    pub url: Option<String>,
    pub timeout: u64,
}
```

`url` is `Option<String>` on `DoclingConfig` too — docling is optional infrastructure.
`timeout` defaults to 600 (seconds).

**Step 2: Wire into Settings and Config**

- Add `pub docling: Option<DoclingSettings>` to `Settings` (after `web`)
- Add `pub docling: DoclingConfig` to `Config` (after `web`)
- Remove `pub docling_url: Option<String>` from `WebSettings`
- Remove `pub docling_url: Option<String>` from `WebConfig`

**Step 3: Update `Config::from_settings`**

Remove the `docling_url` resolution from the `web:` block (lines 418-422, 447).
Add docling resolution:

```rust
docling: {
    let url = settings
        .docling
        .as_ref()
        .and_then(|d| d.url.clone())
        .or_else(|| env::var("DOCLING_URL").ok());
    let timeout = settings
        .docling
        .as_ref()
        .and_then(|d| d.timeout)
        .unwrap_or(600);
    DoclingConfig { url, timeout }
},
```

**Step 4: Update `test_config()`**

Add `docling: DoclingConfig { url: None, timeout: 600 }` to the test config.
Remove `docling_url: None` from the `web:` block.

**Step 5: Update `empty_settings()`**

Add `docling: None` to the `Settings` struct literal.

**Step 6: Run `just ci` to find all compile errors**

Run: `just ci`
Expected: compile errors in files that reference `config.web.docling_url` — these are
fixed in subsequent tasks.

**Step 7: Commit**

```
feat: add [docling] config section, remove [web].docling_url
```

---

### Task 2: Async docling client

**Files:**
- Rewrite: `src/web/docling.rs`
- Modify: `src/web/types.rs` (new error variants)

**Step 1: Add error variants to WebError**

In `src/web/types.rs`, add to `WebError`:

```rust
#[error("docling conversion timed out after {seconds}s")]
DoclingTimeout { seconds: u64 },

#[error("docling task failed: {detail}")]
DoclingTaskFailed { detail: String },
```

Keep the existing `Docling(String)` variant for HTTP/parse errors.

**Step 2: Rewrite `src/web/docling.rs`**

Replace entire file with:

```rust
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::json;

use crate::config::DoclingConfig;

use super::WebError;

/// What to convert.
pub enum DoclingSource<'a> {
    File { path: &'a Path },
    Url { url: &'a str },
}

/// Caller-facing options. Hardcoded defaults (ocr_engine, table_mode,
/// image_export_mode) are set internally.
pub struct ConvertOptions {
    pub ocr: bool,
    pub page_range: Option<(u32, u32)>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            ocr: true,
            page_range: None,
        }
    }
}

/// Single entry point for docling conversion (file or URL → markdown).
#[tracing::instrument(name = "docling convert", skip_all)]
pub async fn convert(
    config: &DoclingConfig,
    source: DoclingSource<'_>,
    options: &ConvertOptions,
) -> Result<String, WebError> {
    let base_url = config.url.as_deref().ok_or_else(|| {
        WebError::Docling("docling URL not configured ([docling].url)".into())
    })?;
    let timeout = Duration::from_secs(config.timeout);

    // Build source JSON
    let source_json = match &source {
        DoclingSource::File { path } => {
            let file_bytes = tokio::fs::read(path)
                .await
                .map_err(|e| WebError::Docling(format!("failed to read file: {e}")))?;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file");
            json!({"kind": "file", "base64_string": b64, "filename": filename})
        }
        DoclingSource::Url { url } => {
            json!({"kind": "http", "url": url})
        }
    };

    // Build options JSON
    let mut opts = json!({
        "to_formats": ["md"],
        "image_export_mode": "placeholder",
        "pipeline": "standard",
        "do_ocr": options.ocr,
        "ocr_engine": "rapidocr",
        "table_mode": "accurate",
    });
    if let Some((start, end)) = options.page_range {
        opts["page_range"] = json!([start, end]);
    }

    let payload = json!({
        "sources": [source_json],
        "options": opts,
    });

    let client = reqwest::Client::new();

    // 1. Submit
    let resp = client
        .post(format!("{base_url}/v1/convert/source/async"))
        .json(&payload)
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("submit failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("submit HTTP {status}: {body}")));
    }

    let submit_body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid submit response: {e}")))?;
    let task_id = submit_body["task_id"]
        .as_str()
        .ok_or_else(|| WebError::Docling("missing task_id in submit response".into()))?
        .to_string();

    // 2. Poll until terminal
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(WebError::DoclingTimeout {
                seconds: config.timeout,
            });
        }

        let poll_resp = client
            .get(format!("{base_url}/v1/status/poll/{task_id}?wait=5"))
            .send()
            .await
            .map_err(|e| WebError::Docling(format!("poll failed: {e}")))?;

        let poll_body: serde_json::Value = poll_resp
            .json()
            .await
            .map_err(|e| WebError::Docling(format!("invalid poll response: {e}")))?;

        let status = poll_body["task_status"]
            .as_str()
            .unwrap_or("unknown");

        match status {
            "success" => break,
            "failure" | "error" => {
                let detail = poll_body["error_message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_string();
                return Err(WebError::DoclingTaskFailed { detail });
            }
            _ => continue, // "pending", "started", etc.
        }
    }

    // 3. Fetch result
    let result_resp = client
        .get(format!("{base_url}/v1/result/{task_id}"))
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("result fetch failed: {e}")))?;

    if !result_resp.status().is_success() {
        let status = result_resp.status();
        let body = result_resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("result HTTP {status}: {body}")));
    }

    let body: serde_json::Value = result_resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid result JSON: {e}")))?;

    extract_markdown_from_response(&body)
}

fn extract_markdown_from_response(body: &serde_json::Value) -> Result<String, WebError> {
    // Try /document/md_content (single-doc response)
    if let Some(md) = body
        .pointer("/document/md_content")
        .and_then(|v| v.as_str())
    {
        return Ok(md.to_string());
    }
    // Try /output/documents/0/md_content (async multi-doc response)
    if let Some(md) = body
        .pointer("/output/documents/0/md_content")
        .and_then(|v| v.as_str())
    {
        return Ok(md.to_string());
    }
    Err(WebError::Docling(format!(
        "could not extract markdown from response: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    )))
}
```

**Step 3: Add `base64` dependency if not already present**

Run: `grep 'base64' Cargo.toml`

If not present: `cargo add base64`

**Step 4: Run `just ci`**

Expected: compile errors in `import_page`, `import_file`, and `cli/reference.rs` because
the old `convert_file`/`convert_url` signatures no longer exist. Fixed in next tasks.

**Step 5: Commit**

```
feat: rewrite docling client to use async API with polling
```

---

### Task 3: Update callers — import_page and import_file

**Files:**
- Modify: `src/reference_import/page.rs`
- Modify: `src/reference_import/file.rs`
- Modify: `src/reference_import/mod.rs`
- Modify: `src/reference_import/types.rs`

**Step 1: Update `import_page` signature and body**

In `src/reference_import/page.rs`:

- Change import: `use crate::config::WebConfig` → `use crate::config::DoclingConfig`
- Change parameter: `web_config: &WebConfig` → `docling_config: &DoclingConfig`
- Update the docling fallback path (lines 58-67):

```rust
Err(web::WebError::UnsupportedContentType { .. }) => {
    web::docling::convert(
        docling_config,
        web::docling::DoclingSource::Url { url },
        &web::docling::ConvertOptions::default(),
    )
    .await
    .map_err(|e| ImportError::Fetch(e.to_string()))?
}
```

Note: the `web::fetch` call on line 56 does NOT use web_config — it takes
`crawl4ai_url` which is passed as `None` for page imports. So removing `&WebConfig`
from the signature is clean.

**Step 2: Update `import_file` signature and body**

In `src/reference_import/file.rs`:

- Change import: `use crate::config::WebConfig` → `use crate::config::DoclingConfig`
- Change parameter: `web_config: &WebConfig` → `docling_config: &DoclingConfig`
- Replace the docling call (lines 21-24, 74-75):

```rust
let markdown = crate::web::docling::convert(
    docling_config,
    crate::web::docling::DoclingSource::File { path: &source_path },
    &crate::web::docling::ConvertOptions::default(),
)
.await
.map_err(|e| ImportError::Fetch(e.to_string()))?;
```

Remove the old `docling_url` extraction at the top of the function.

**Step 3: Update `cli/reference.rs` call sites**

Change `&config.web` to `&config.docling` in the two call sites (lines 164, 170):

```rust
ImportSource::Page { .. } => {
    crate::reference_import::import_page(db, workspace, &config.docling, &import_config).await?
}
// ...
ImportSource::File { .. } => {
    crate::reference_import::import_file(db, workspace, &config.docling, &import_config).await?
}
```

**Step 4: Run `just ci`**

Expected: PASS (all compile errors resolved).

**Step 5: Commit**

```
refactor: update import_page/import_file to use new docling client
```

---

### Task 4: Options plumbing — `--no-ocr`, `--page-range`, `--timeout`

**Files:**
- Modify: `src/reference_import/types.rs`
- Modify: `src/reference_import/page.rs`
- Modify: `src/reference_import/file.rs`

**Step 1: Add docling options to ImportSource variants**

In `src/reference_import/types.rs`, add fields to `Page` and `File`:

```rust
Page {
    url: String,
    no_ocr: bool,
    page_range: Option<(u32, u32)>,
},
File {
    path: String,
    no_ocr: bool,
    page_range: Option<(u32, u32)>,
},
```

**Step 2: Build ConvertOptions from ImportSource fields**

In `import_page` (the docling fallback path):

```rust
let convert_opts = web::docling::ConvertOptions {
    ocr: !no_ocr,
    page_range: *page_range,
};
```

(Where `no_ocr` and `page_range` are destructured from `ImportSource::Page`.)

Same in `import_file`.

**Step 3: Update all ImportSource construction sites**

In `src/cli/reference.rs`, add the new fields with defaults when constructing
`ImportSource::Page` and `ImportSource::File`:

```rust
"page" => ImportSource::Page {
    url: require_url()?.to_string(),
    no_ocr: false,
    page_range: None,
},
"file" => {
    // ...
    ImportSource::File {
        path: path.to_string(),
        no_ocr: false,
        page_range: None,
    }
}
```

These defaults will be replaced with real CLI args in Task 5.

**Step 4: Run `just ci`**

Expected: PASS.

**Step 5: Commit**

```
feat: plumb docling options (no_ocr, page_range) through import types
```

---

### Task 5: CLI split — `ghost reference import git|crawl` + `ghost document import url|file`

**Files:**
- Rewrite: `src/cli/reference.rs`
- Create: `src/cli/document.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`
- Modify: `src/tools/web_fetch.rs` (error message update)

**Step 1: Rewrite `src/cli/reference.rs`**

Replace the single `Import` variant with clap subcommands:

```rust
use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportConfig, ImportSource};

#[derive(Debug, Subcommand)]
pub enum ReferenceCommand {
    /// Import references from external sources
    Import {
        #[command(subcommand)]
        command: ReferenceImportCommand,
    },
    /// Delete a topic and all its references
    Delete {
        #[arg(long)]
        topic: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReferenceImportCommand {
    /// Import from a git repository
    Git {
        #[arg(long)]
        url: String,
        #[arg(long)]
        topic: String,
        #[arg(long, value_delimiter = ',')]
        paths: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        extensions: Vec<String>,
    },
    /// Import by crawling a website
    Crawl {
        #[arg(long)]
        url: String,
        #[arg(long)]
        topic: String,
        #[arg(long, default_value_t = 3)]
        max_depth: usize,
        #[arg(long, default_value_t = 50)]
        max_pages: usize,
    },
}
```

Update `execute()` to match the new structure. The `cmd_import` function simplifies
since it no longer needs to parse a string source type. Keep `cmd_delete` as-is.

```rust
pub async fn execute(command: ReferenceCommand) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
    let workspace = std::path::Path::new(&config.workspace);

    match command {
        ReferenceCommand::Import { command } => match command {
            ReferenceImportCommand::Git {
                url,
                topic,
                paths,
                extensions,
            } => {
                let import_config = ImportConfig {
                    source: ImportSource::Git { url: url.clone(), paths, extensions },
                    topic: topic.clone(),
                };
                println!("Importing from git: {url}");
                println!("Topic: {topic}");
                let result =
                    crate::reference_import::import_git(&db, workspace, &import_config).await?;
                print_result(&topic, "git", Some(&url), result);
                Ok(())
            }
            ReferenceImportCommand::Crawl {
                url,
                topic,
                max_depth,
                max_pages,
            } => {
                let import_config = ImportConfig {
                    source: ImportSource::Crawl {
                        url: url.clone(),
                        max_depth,
                        max_pages,
                    },
                    topic: topic.clone(),
                };
                println!("Importing from crawl: {url}");
                println!("Topic: {topic}");
                let result =
                    crate::reference_import::import_crawl(&db, workspace, &import_config).await?;
                print_result(&topic, "crawl", Some(&url), result);
                Ok(())
            }
        },
        ReferenceCommand::Delete { topic } => cmd_delete(&db, workspace, &topic).await,
    }
}
```

Extract `print_result` helper from the old `cmd_import` output logic (lines 174-197):

```rust
fn print_result(
    topic: &str,
    source: &str,
    url: Option<&str>,
    result: crate::reference_import::ImportResult,
) {
    println!(
        "Done. Created: {}, Skipped: {}",
        result.references_created, result.references_skipped
    );
    if result.references_created > 0 {
        let ref_dir = format!("references/{topic}/");
        match source {
            "page" | "file" | "url" => println!("Reference saved to: {ref_dir}"),
            _ => println!("References saved to: {ref_dir}"),
        }
        println!("Embeddings are being computed in the background by the file watcher.");
        println!(
            "\n  NOTE: A skeleton index note exists at notes/{topic}/index.md\n  \
             It may only contain a placeholder description.\n  \
             Edit it with a real description of what this library/topic is about —\n  \
             semantic search relies on this to discover the topic."
        );
    }
}
```

Keep `cmd_delete` unchanged.

**Step 2: Create `src/cli/document.rs`**

```rust
use clap::Subcommand;

use crate::db;
use crate::error::GhostError;
use crate::reference_import::{ImportConfig, ImportSource};

#[derive(Debug, Subcommand)]
pub enum DocumentCommand {
    /// Import a document (PDF, DOCX, etc.)
    Import {
        #[command(subcommand)]
        command: DocumentImportCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DocumentImportCommand {
    /// Import a document from a URL
    Url {
        #[arg(long)]
        url: String,
        #[arg(long)]
        topic: String,
        /// Disable OCR (faster for digital PDFs)
        #[arg(long, default_value_t = false)]
        no_ocr: bool,
        /// Page range, e.g. "1-10" (default: full document)
        #[arg(long)]
        page_range: Option<String>,
        /// Timeout in seconds (default: from config, usually 600)
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Import a document from a local file
    File {
        #[arg(long)]
        path: String,
        #[arg(long)]
        topic: String,
        /// Disable OCR (faster for digital PDFs)
        #[arg(long, default_value_t = false)]
        no_ocr: bool,
        /// Page range, e.g. "1-10" (default: full document)
        #[arg(long)]
        page_range: Option<String>,
        /// Timeout in seconds (default: from config, usually 600)
        #[arg(long)]
        timeout: Option<u64>,
    },
}

#[tracing::instrument(name = "execute document_command", skip_all)]
pub async fn execute(command: DocumentCommand) -> Result<(), GhostError> {
    let _observability = crate::observability::init()?;
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;
    let db = crate::db::connect(&config.workspace, config.embeddings.dimension).await?;
    let workspace = std::path::Path::new(&config.workspace);

    match command {
        DocumentCommand::Import { command } => match command {
            DocumentImportCommand::Url {
                url,
                topic,
                no_ocr,
                page_range,
                timeout,
            } => {
                let page_range = parse_page_range(page_range.as_deref())?;
                let mut docling_config = config.docling.clone();
                if let Some(t) = timeout {
                    docling_config.timeout = t;
                }
                let import_config = ImportConfig {
                    source: ImportSource::Page {
                        url: url.clone(),
                        no_ocr,
                        page_range,
                    },
                    topic: topic.clone(),
                };
                println!("Importing document from URL: {url}");
                println!("Topic: {topic}");
                let result = crate::reference_import::import_page(
                    &db,
                    workspace,
                    &docling_config,
                    &import_config,
                )
                .await?;
                print_result(&topic, result);
                Ok(())
            }
            DocumentImportCommand::File {
                path,
                topic,
                no_ocr,
                page_range,
                timeout,
            } => {
                let page_range = parse_page_range(page_range.as_deref())?;
                let mut docling_config = config.docling.clone();
                if let Some(t) = timeout {
                    docling_config.timeout = t;
                }
                let import_config = ImportConfig {
                    source: ImportSource::File {
                        path: path.clone(),
                        no_ocr,
                        page_range,
                    },
                    topic: topic.clone(),
                };
                println!("Importing document from file: {path}");
                println!("Topic: {topic}");
                let result = crate::reference_import::import_file(
                    &db,
                    workspace,
                    &docling_config,
                    &import_config,
                )
                .await?;
                print_result(&topic, result);
                Ok(())
            }
        },
    }
}

fn parse_page_range(s: Option<&str>) -> Result<Option<(u32, u32)>, GhostError> {
    let Some(s) = s else { return Ok(None) };
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return Err(GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid page range '{s}', expected format: '1-10'"),
        )));
    }
    let start: u32 = parts[0].trim().parse().map_err(|_| {
        GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid page range start: '{}'", parts[0]),
        ))
    })?;
    let end: u32 = parts[1].trim().parse().map_err(|_| {
        GhostError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid page range end: '{}'", parts[1]),
        ))
    })?;
    Ok(Some((start, end)))
}

fn print_result(topic: &str, result: crate::reference_import::ImportResult) {
    println!(
        "Done. Created: {}, Skipped: {}",
        result.references_created, result.references_skipped
    );
    if result.references_created > 0 {
        let ref_dir = format!("references/{topic}/");
        println!("Reference saved to: {ref_dir}");
        println!("Embeddings are being computed in the background by the file watcher.");
        println!(
            "\n  NOTE: A skeleton index note exists at notes/{topic}/index.md\n  \
             It may only contain a placeholder description.\n  \
             Edit it with a real description of what this library/topic is about —\n  \
             semantic search relies on this to discover the topic."
        );
    }
}
```

**Step 3: Register the new module and command**

In `src/cli/mod.rs`, add:
```rust
pub mod document;
```

In `src/main.rs`, add to `Commands` enum:
```rust
Document {
    #[command(subcommand)]
    command: ghost::cli::document::DocumentCommand,
},
```

Add to `dispatch()`:
```rust
Commands::Document { command } => ghost::cli::document::execute(command).await,
```

**Step 4: Update web_fetch error message**

In `src/tools/web_fetch.rs` (around line 93), change:
```
`ghost reference import --source page --url '{url}' --topic <name>`
```
to:
```
`ghost document import url --url '{url}' --topic <name>`
```

**Step 5: Run `just ci`**

Expected: PASS.

**Step 6: Commit**

```
feat: split CLI into reference import (git/crawl) and document import (url/file)
```

---

### Task 6: Skills — write `document-import`, trim `reference-import`

**Files:**
- Create: `prompts/skills/document-import.md`
- Modify: `prompts/skills/reference-import.md`

**Step 1: Create `prompts/skills/document-import.md`**

```markdown
---
name: document-import
description:
  Import documents (PDF, DOCX, XLSX, PPTX, images) into the knowledge base via
  docling-serve. Use when the OPERATOR asks to import a document from a URL or
  uploaded file, when web_fetch returns an unsupported content type, or when you
  need to import non-HTML content as a searchable reference.
---

# Document Import Skill

Import documents (PDF, DOCX, etc.) as topic-scoped references via docling-serve.

## Decision Flow

1. **Search first**: `knowledge_search(query="<topic>", categories=["references"])`.
   If results exist, use them. Done.
2. **URL source**: use `ghost document import url --url <url> --topic <name>` with
   `background: true`.
3. **File upload**: if the OPERATOR uploaded a file, import with
   `ghost document import file --path uploads/<filename> --topic <name>` with
   `background: true`.
4. **After starting the import**: tell the OPERATOR it's importing, include any other
   pending responses, then **end your turn**. A follow-up turn is triggered
   automatically when the import completes — you'll see the
   `[shell-command completed]` system message. Search the imported refs and answer.

## CLI Commands

```
ghost document import url --url <url> --topic <name>
ghost document import file --path <path> --topic <name>
```

### OPERATOR-facing options (use ONLY when explicitly requested)

These are optimization overrides. **Use defaults unless the OPERATOR asks otherwise.**

| Flag                  | Default  | When to use                               |
| --------------------- | -------- | ----------------------------------------- |
| `--no-ocr`            | OCR on   | OPERATOR says PDF is digital, wants speed |
| `--page-range "1-10"` | full doc | OPERATOR wants specific pages only        |
| `--timeout 900`       | 600s     | OPERATOR needs more time for huge docs    |

Do NOT guess at these options. Do NOT add `--no-ocr` to "speed things up". The OPERATOR
will tell you if they want non-default behavior.

## Running the Import (Background)

Document imports can take 1-2 minutes for a typical PDF. **Always use background mode**:

```json
{
  "command": "ghost document import url --url https://example.com/rulebook.pdf --topic boardgames/arknova",
  "background": true
}
```

Tell the OPERATOR: _"I'm importing the document in the background — I'll search it once
the import finishes."_ Then **end your turn**.

## File Import (Uploaded Files)

When the OPERATOR uploads a file, it lands in `uploads/` in the workspace:

```json
{
  "command": "ghost document import file --path uploads/<filename> --topic <topic-name>",
  "background": true
}
```

The original file is preserved in `references/<topic>/_originals/`. After import, clean
up the uploaded file — `uploads/` is a transient inbox:

```json
{
  "command": "rm uploads/<filename>"
}
```

## Post-Import: Enrich the Topic Note

After import, a placeholder note exists at `notes/<topic>/index.md`. Edit it with a
meaningful description — what the document covers, key concepts. This makes the topic
discoverable via semantic search.

## Post-Import Search

```
knowledge_search(query="setup procedure", topic="boardgames/arknova", categories=["references"])
```

## Cleanup

```
ghost reference delete --topic boardgames/arknova
```
```

**Step 2: Trim `prompts/skills/reference-import.md`**

Remove the "page" and "file" source types from the decision flow and CLI commands.
Remove the "File Import" section. Update the description to only cover git and crawl.

The skill should reference `document-import` for PDF/DOCX/file imports:

Add near the top of the decision flow:
```
2. **Document import** (PDF, DOCX, uploaded file): use the `document-import` skill
   instead. This skill covers git repos and web crawls only.
```

Remove from CLI commands:
```
ghost reference import --source page --url <url> --topic <name>
ghost reference import --source file --path <path> --topic <name>
```

Update remaining commands to new syntax:
```
ghost reference import git --url <url> --topic <name> \
    [--paths dir1,dir2] [--extensions .md,.rs]

ghost reference import crawl --url <url> --topic <name> \
    [--max-depth 3] [--max-pages 50]
```

Remove the "File Import (Uploaded Files)" section entirely.

Update the decision flow step for single URLs: remove the step about
`--source page` routing through docling. Instead point to `document-import` skill.

**Step 3: Update other skills that reference the old CLI syntax**

Check `prompts/skills/deep-research/skill.md` and
`prompts/skills/knowledge-navigator/skill.md` for references to the old
`ghost reference import --source page` or `--source file` syntax. Update to point to
`ghost document import url` or `ghost document import file`.

**Step 4: Run `just ci`**

Expected: PASS (skills are just markdown, but verify no Rust broke).

**Step 5: Commit**

```
feat: add document-import skill, update reference-import for git/crawl only
```

---

### Task 7: Update docker-compose and config references

**Files:**
- Modify: `docker-compose.yml` (at project root)
- Modify: `deploy/macos/docker-compose.yml` (if exists)

**Step 1: Update `docker-compose.yml`**

The `DOCLING_URL` env var is still respected by config.rs (env fallback). No changes
needed to the compose files for functionality, but verify the env var name matches.

**Step 2: Run the e2e test**

Run: `cargo test --features live-tests ark_nova_step_01 -- --nocapture`

This test exercises the full pipeline: GHOST reads skill → calls
`ghost reference import --source page` → docling converts → search. It should now use
the new `ghost document import url` path (via the updated skill).

Note: this test requires a running docling-serve instance. If not available, verify
manually with:

```
ghost document import url --url https://example.com/test.pdf --topic test/doc
```

**Step 3: Run `just ci` one final time**

Expected: all green.

**Step 4: Commit any remaining fixes**

```
chore: update compose files and verify e2e compatibility
```
