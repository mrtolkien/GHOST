# Complex Reference Format Ingestion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Enable Ghost to import non-text formats (PDF, DOCX, XLSX, images) as markdown
references using docling-serve, with full upload support from Discord.

**Architecture:** Add docling-serve as a Docker service (CPU, port 5001). Extend
reference import with a `--source file` path that converts local files via docling's REST
API. Rework Discord attachment handling to download all file types to `uploads/`.

**Tech Stack:** docling-serve (Docker), reqwest multipart, existing reference import
pipeline.

---

## Task 1: Add docling-serve to Docker Compose

**Files:**
- Modify: `docker-compose.yml`

**Step 1: Add the service**

Add `docling-serve` after the `searxng` service:

```yaml
  docling-serve:
    image: ghcr.io/docling-project/docling-serve-cpu:latest
    networks:
      - ghost-net
    restart: unless-stopped
```

**Step 2: Add env var to ghost service**

Add `DOCLING_URL=http://docling-serve:5001` to the ghost service's environment list,
alongside the existing `CRAWL4AI_URL` and `SEARXNG_URL`.

**Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "feat: add docling-serve to docker-compose"
```

---

## Task 2: Add `docling_url` to config

**Files:**
- Modify: `src/config.rs`

**Step 1: Add to WebSettings** (line 144)

Add `pub docling_url: Option<String>` to `WebSettings`:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub search_max_results: Option<usize>,
    pub crawl4ai_url: Option<String>,
    pub docling_url: Option<String>,
    pub search: Option<SearchProviderSettings>,
}
```

**Step 2: Add to WebConfig** (line 231)

Add `pub docling_url: Option<String>` to `WebConfig`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct WebConfig {
    pub search_max_results: usize,
    pub crawl4ai_url: Option<String>,
    pub docling_url: Option<String>,
    pub search_provider: SearchProviderConfig,
}
```

**Step 3: Add resolution logic** (around line 396)

After the `crawl4ai_url` resolution block, add:

```rust
let docling_url = settings
    .web
    .as_ref()
    .and_then(|w| w.docling_url.clone())
    .or_else(|| env::var("DOCLING_URL").ok());
```

And include it in the `WebConfig` construction:

```rust
WebConfig {
    search_max_results: ...,
    crawl4ai_url,
    docling_url,
    search_provider,
}
```

**Step 4: Run `just ci` to verify**

**Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add docling_url config (env DOCLING_URL)"
```

---

## Task 3: Create docling client module

**Files:**
- Create: `src/web/docling.rs`
- Modify: `src/web/mod.rs`
- Modify: `src/web/types.rs` (if WebError needs a new variant)
- Modify: `Cargo.toml` (add `multipart` feature to reqwest)

**Step 1: Add `multipart` feature to reqwest in `Cargo.toml`**

```toml
reqwest = { version = "0.13", default-features = false, features = [
  "json",
  "rustls",
  "query",
  "form",
  "multipart",
] }
```

**Step 2: Add a `Docling` variant to `WebError`**

Check `src/web/types.rs` for the `WebError` enum. Add:

```rust
#[error("docling conversion failed: {0}")]
Docling(String),
```

**Step 3: Create `src/web/docling.rs`**

This module wraps docling-serve's REST API. Two functions:

```rust
use std::path::Path;
use reqwest::multipart;
use super::WebError;

/// Convert a local file to markdown via docling-serve's /v1/convert/file endpoint.
#[tracing::instrument(name = "docling convert file", skip_all, fields(path = %path.display()))]
pub async fn convert_file(
    docling_url: &str,
    path: &Path,
) -> Result<String, WebError> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let file_bytes = tokio::fs::read(path)
        .await
        .map_err(|e| WebError::Docling(format!("failed to read file: {e}")))?;

    let part = multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("application/octet-stream")
        .map_err(|e| WebError::Docling(e.to_string()))?;

    let form = multipart::Form::new()
        .part("files", part)
        .text("options", serde_json::json!({"to_formats": ["md"]}).to_string());

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{docling_url}/v1/convert/file"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("HTTP {status}: {body}")));
    }

    // Response is JSON with document content — extract markdown
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid JSON response: {e}")))?;

    extract_markdown_from_response(&body)
}

/// Convert a URL-hosted document to markdown via docling-serve's /v1/convert/source.
#[tracing::instrument(name = "docling convert url", skip_all, fields(%url))]
pub async fn convert_url(
    docling_url: &str,
    url: &str,
) -> Result<String, WebError> {
    let payload = serde_json::json!({
        "http_sources": [{"url": url}],
        "options": {"to_formats": ["md"]}
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{docling_url}/v1/convert/source"))
        .json(&payload)
        .send()
        .await
        .map_err(|e| WebError::Docling(format!("request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(WebError::Docling(format!("HTTP {status}: {body}")));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WebError::Docling(format!("invalid JSON response: {e}")))?;

    extract_markdown_from_response(&body)
}

/// Extract markdown text from docling-serve's JSON response.
/// The response structure contains a `document` with export results.
fn extract_markdown_from_response(body: &serde_json::Value) -> Result<String, WebError> {
    // docling-serve returns: { "document": { "md_content": "..." } }
    // or possibly nested differently — we need to inspect the actual response
    // and adapt. Start with the most likely structure.
    if let Some(md) = body.pointer("/document/md_content").and_then(|v| v.as_str()) {
        return Ok(md.to_string());
    }
    // Alternative: array of documents
    if let Some(docs) = body.get("documents").and_then(|v| v.as_array()) {
        if let Some(first) = docs.first() {
            if let Some(md) = first.pointer("/md_content").and_then(|v| v.as_str()) {
                return Ok(md.to_string());
            }
        }
    }
    Err(WebError::Docling(format!(
        "could not extract markdown from response: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    )))
}
```

> **Note for implementer:** The exact JSON response structure of docling-serve needs
> verification. Spin up docling-serve locally and test with a sample PDF to confirm the
> response shape. Adjust `extract_markdown_from_response` accordingly. Use the `/docs`
> endpoint at `http://localhost:5001/docs` for the OpenAPI schema.

**Step 4: Export from `src/web/mod.rs`**

Add:
```rust
mod docling;
pub use docling::{convert_file, convert_url};
```

**Step 5: Run `just ci`**

**Step 6: Commit**

```bash
git add Cargo.toml src/web/docling.rs src/web/mod.rs src/web/types.rs
git commit -m "feat: docling client for file and URL conversion"
```

---

## Task 4: Add `File` variant to import source + CLI

**Files:**
- Modify: `src/reference_import/types.rs`
- Modify: `src/cli/reference.rs`

**Step 1: Add `File` variant to `ImportSource`**

```rust
#[derive(Debug)]
pub enum ImportSource {
    Git {
        url: String,
        paths: Vec<String>,
        extensions: Vec<String>,
    },
    Page {
        url: String,
    },
    Crawl {
        url: String,
        max_depth: usize,
        max_pages: usize,
    },
    File {
        path: String,
    },
}
```

**Step 2: Add `--path` arg and `"file"` source to CLI**

In `src/cli/reference.rs`, add the `--path` argument to the `Import` variant:

```rust
/// Local file path (file import only)
#[arg(long)]
path: Option<String>,
```

Make `url` optional (it's not needed for file imports):

```rust
/// URL to import from (not needed for file imports)
#[arg(long)]
url: Option<String>,
```

In `cmd_import`, update the signature to take `url: Option<&str>` and `path: Option<&str>`,
and add the `"file"` match arm:

```rust
"file" => {
    let path = path.ok_or_else(|| GhostError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "--path is required for file imports",
    )))?;
    ImportSource::File {
        path: path.to_string(),
    }
}
```

The git/page/crawl arms should error if `url` is `None`.

Add the dispatch arm in the match block:

```rust
ImportSource::File { .. } => {
    crate::reference_import::import_file(
        db, workspace, &config.embeddings, &config.web, &import_config,
    )
    .await?
}
```

Note: `import_file` needs `&WebConfig` to read `docling_url`. The other import functions
don't need it, so only pass it for `File`.

**Step 3: Run `just ci`** (will fail until Task 5 implements `import_file` — that's OK,
just verify the types compile with `cargo check`)

**Step 4: Commit**

```bash
git add src/reference_import/types.rs src/cli/reference.rs
git commit -m "feat: add File import source variant and --path CLI arg"
```

---

## Task 5: Implement `import_file`

**Files:**
- Create: `src/reference_import/file.rs`
- Modify: `src/reference_import/mod.rs`

**Step 1: Create `src/reference_import/file.rs`**

Follow the same pattern as `page.rs` but using the docling client:

```rust
use std::path::Path;

use crate::config::{EmbeddingsConfig, WebConfig};
use crate::db;
use crate::db::GhostDb;
use crate::embeddings::EmbeddingClient;
use crate::embeddings::pipeline::{EmbedRequest, embed_sources};
use crate::web;

use super::topic::ensure_topic_hierarchy;
use super::types::{ImportConfig, ImportError, ImportResult, ImportSource};

/// Import a local file as a reference, converting via docling-serve.
#[tracing::instrument(name = "import file", skip_all, fields(topic = %config.topic))]
pub async fn import_file(
    db: &GhostDb,
    workspace: &Path,
    embeddings_config: &EmbeddingsConfig,
    web_config: &WebConfig,
    config: &ImportConfig,
) -> Result<ImportResult, ImportError> {
    let ImportSource::File { path: file_path } = &config.source else {
        return Err(ImportError::Fetch("expected file source".into()));
    };

    let docling_url = web_config
        .docling_url
        .as_deref()
        .ok_or_else(|| ImportError::Fetch("docling_url not configured".into()))?;

    // Resolve file path (relative to workspace or absolute)
    let source_path = if Path::new(file_path).is_absolute() {
        std::path::PathBuf::from(file_path)
    } else {
        workspace.join(file_path)
    };

    if !source_path.exists() {
        return Err(ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", source_path.display()),
        )));
    }

    let original_filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let stem = source_path
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    // Ensure topic hierarchy
    let topic_id = ensure_topic_hierarchy(db, &config.topic).await?;

    // Build ref path
    let filename = format!("{stem}.md");
    let ref_path = format!("{}/{filename}", config.topic);

    // Idempotency: skip if already imported
    if db::knowledge::find_reference_by_path(db, &ref_path)
        .await?
        .is_some()
    {
        let batch_id = db::knowledge::upsert_import_batch(
            db, &topic_id, "file", file_path, None, 1,
        )
        .await?;
        return Ok(ImportResult {
            topic_id,
            batch_id,
            references_created: 0,
            references_skipped: 1,
            embeddings_generated: 0,
        });
    }

    // Upsert import batch
    let batch_id = db::knowledge::upsert_import_batch(
        db, &topic_id, "file", file_path, None, 0,
    )
    .await?;

    // Convert via docling-serve
    let markdown = web::convert_file(docling_url, &source_path)
        .await
        .map_err(|e| ImportError::Fetch(e.to_string()))?;

    // Preserve original in _originals/
    let originals_dir = workspace
        .join("references")
        .join(&config.topic)
        .join("_originals");
    std::fs::create_dir_all(&originals_dir)?;
    std::fs::copy(&source_path, originals_dir.join(&original_filename))?;

    // Write extracted markdown to disk
    let disk_path = workspace
        .join("references")
        .join(&config.topic)
        .join(&filename);
    if let Some(parent) = disk_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&disk_path, &markdown)?;

    // Store as reference in DB
    let ref_id = db::knowledge::create_reference(
        db,
        &topic_id,
        &ref_path,
        &markdown,
        None, // no source URL for local files
        Some(&batch_id),
    )
    .await?;

    // Embed
    let client = EmbeddingClient::new(embeddings_config);
    let embed_requests = vec![EmbedRequest {
        source_table: "reference".into(),
        source_id: ref_id,
        content: markdown,
        tags: vec![config.topic.clone()],
        topic_id: Some(topic_id.clone()),
        path: Some(ref_path.clone()),
    }];
    let embeddings_generated = embed_sources(&client, db, embed_requests).await?;

    // Update batch with final ref count
    let total_refs =
        db::knowledge::count_references_by_topic(db, &topic_id).await? as usize;
    let batch_id = db::knowledge::upsert_import_batch(
        db, &topic_id, "file", file_path, None, total_refs as i64,
    )
    .await?;

    // Write _import.toml and ensure index notes
    super::topic::write_import_toml(
        workspace,
        &config.topic,
        "file",
        file_path,
        None,
        total_refs,
    )?;

    Ok(ImportResult {
        topic_id,
        batch_id,
        references_created: 1,
        references_skipped: 0,
        embeddings_generated,
    })
}
```

**Step 2: Export from `src/reference_import/mod.rs`**

Add:
```rust
mod file;
pub use file::import_file;
```

**Step 3: Run `just ci`**

**Step 4: Commit**

```bash
git add src/reference_import/file.rs src/reference_import/mod.rs
git commit -m "feat: implement import_file with docling conversion"
```

---

## Task 6: Rework Discord attachment handling (downloads -> uploads)

**Files:**
- Modify: `src/interfaces/discord/bot.rs`

**Step 1: Rename `downloads` to `uploads`**

Change the constant/directory name from `"downloads"` to `"uploads"` in
`process_attachments()` (line 145):

```rust
let upload_dir = self.config.workspace.join("uploads");
```

**Step 2: Download all file types, not just text**

Rework `process_attachments` to always download, but vary the message format:

```rust
async fn process_attachments(
    &self,
    attachments: &[serenity::model::channel::Attachment],
) -> String {
    if attachments.is_empty() {
        return String::new();
    }

    let upload_dir = self.config.workspace.join("uploads");
    if let Err(e) = tokio::fs::create_dir_all(&upload_dir).await {
        error!("Failed to create uploads dir: {e}");
        return String::new();
    }

    let timestamp = chrono::Utc::now().format("%s");
    let client = reqwest::Client::new();
    let mut lines = Vec::new();

    for attachment in attachments {
        let ext = attachment
            .filename
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_lowercase();

        let is_text = TEXT_EXTENSIONS.contains(&ext.as_str());

        let dest_name = format!("{timestamp}_{}", attachment.filename);
        let dest_path = upload_dir.join(&dest_name);

        match client.get(&attachment.url).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(bytes) => {
                    if bytes.len() > MAX_ATTACHMENT_SIZE {
                        warn!(
                            filename = %attachment.filename,
                            size = bytes.len(),
                            "Attachment exceeds 25MB limit, skipping"
                        );
                        lines.push(format!(
                            "[Attachment too large: {}]",
                            attachment.filename
                        ));
                        continue;
                    }
                    if let Err(e) = tokio::fs::write(&dest_path, &bytes).await {
                        error!("Failed to write attachment {dest_name}: {e}");
                        continue;
                    }
                    if is_text {
                        // Inline text content as before
                        lines.push(format!(
                            "[File uploaded: uploads/{dest_name}]"
                        ));
                    } else {
                        // Non-text: just note the upload path
                        lines.push(format!(
                            "[File uploaded: uploads/{dest_name}]"
                        ));
                    }
                }
                Err(e) => error!(
                    "Failed to download attachment body {}: {e}",
                    attachment.filename
                ),
            },
            Err(e) => error!(
                "Failed to fetch attachment {}: {e}",
                attachment.filename
            ),
        }
    }

    if lines.is_empty() {
        return String::new();
    }
    lines.join("\n")
}
```

> **Note:** The `TEXT_EXTENSIONS` constant can stay for now — it controls whether content
> is inlined in the message or just referenced by path. Both types get downloaded.

**Step 3: Run `just ci`**

**Step 4: Commit**

```bash
git add src/interfaces/discord/bot.rs
git commit -m "refactor: rename downloads to uploads, download all file types"
```

---

## Task 7: Update reference-import skill

**Files:**
- Modify: `prompts/skills/reference-import.md`

**Step 1: Update the skill**

Add file import documentation. Key additions:

1. Update the description to mention file imports
2. Add `--source file` to the CLI commands section
3. Add a new "File Import (Uploaded Files)" section after "Crawl Import"
4. Update the decision flow to include file handling

Add to CLI commands:
```
ghost reference import --source file --path <path> --topic <name>
```

Add new section:
```markdown
## File Import (Uploaded Files)

When the OPERATOR uploads a file (PDF, DOCX, XLSX, images, etc.), it lands in
`uploads/` in the workspace. To import it as a reference:

\```json
{
  "command": "ghost reference import --source file --path uploads/<filename> --topic <topic-name>",
  "background": true
}
\```

Docling handles: PDF, DOCX, XLSX, PPTX, HTML, images (PNG, JPG), and more.
The original file is preserved in `references/<topic>/_originals/`.

After import, the uploaded file can be cleaned up — `uploads/` is a transient inbox.
```

**Step 2: Commit**

```bash
git add prompts/skills/reference-import.md
git commit -m "docs: update reference-import skill with file import workflow"
```

---

## Task 8: Manual integration test with docling-serve

This is NOT an automated test — it's a manual verification step.

**Step 1: Start docling-serve**

```bash
docker compose up docling-serve -d
```

**Step 2: Verify docling-serve is healthy**

```bash
curl http://localhost:5001/docs
```

Should return the OpenAPI docs page.

**Step 3: Test file conversion directly**

```bash
curl -X POST http://localhost:5001/v1/convert/file \
  -F "files=@test.pdf" \
  -F 'options={"to_formats": ["md"]}'
```

Inspect the JSON response structure. If it differs from what
`extract_markdown_from_response` expects, fix the parsing in `src/web/docling.rs`.

**Step 4: Test the full import flow**

```bash
# Place a test PDF in the workspace uploads dir
cp test.pdf ~/GHOST/uploads/

# Run the import
ghost reference import --source file --path uploads/test.pdf --topic test-pdf
```

Verify:
- `references/test-pdf/test.md` exists with extracted markdown
- `references/test-pdf/_originals/test.pdf` exists
- `references/test-pdf/_import.toml` has `source_type = "file"`
- `ghost reference delete --topic test-pdf` cleans up

**Step 5: Test Discord upload flow**

Send a PDF to the GHOST via Discord. Verify:
- File appears in `uploads/`
- Message shows `[File uploaded: uploads/<timestamp>_<filename>]`
- GHOST can be instructed to import it

---

## Summary of All Files Changed

| File | Action |
|------|--------|
| `docker-compose.yml` | Add docling-serve service + env var |
| `Cargo.toml` | Add `multipart` feature to reqwest |
| `src/config.rs` | Add `docling_url` to WebSettings + WebConfig |
| `src/web/docling.rs` | New: docling REST client |
| `src/web/mod.rs` | Export docling functions |
| `src/web/types.rs` | Add `Docling` variant to WebError |
| `src/reference_import/types.rs` | Add `File` variant to ImportSource |
| `src/reference_import/file.rs` | New: import_file implementation |
| `src/reference_import/mod.rs` | Export import_file |
| `src/cli/reference.rs` | Add --path arg, file source, dispatch |
| `src/interfaces/discord/bot.rs` | Rename downloads->uploads, download all types |
| `prompts/skills/reference-import.md` | Document file import workflow |
