//! Tuneable operational constants for the GHOST runtime.
//!
//! Centralizes limits, timeouts, and sizes that affect GHOST's behavior.
//! Module-specific constants (rendering, API endpoints, colors) stay in their
//! respective files — this module is for values you'd realistically want to
//! find and adjust.

use std::time::Duration;

// ---------------------------------------------------------------------------
// Chat & tool execution
// ---------------------------------------------------------------------------

/// Maximum tool loop iterations before the chat loop auto-stops.
pub const DEFAULT_MAX_TOOL_ITERATIONS: usize = 50;

/// Per-request timeout for provider API calls. Providers can hang indefinitely
/// (observed in live tests). This wraps each `Provider::chat()` call.
/// On timeout, the request is retried once before propagating the error.
pub const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Max characters fed into the LLM for context summarization.
pub const MAX_SUMMARIZATION_INPUT_CHARS: usize = 50_000;

/// Minimum assistant message length to qualify as "agent findings".
pub const MIN_FINDINGS_CHARS: usize = 500;

/// Default model context window size (tokens) when not configured.
pub const DEFAULT_CONTEXT_WINDOW: usize = 200_000;

// ---------------------------------------------------------------------------
// Shell execution
// ---------------------------------------------------------------------------

/// Default timeout for foreground shell commands.
pub const DEFAULT_SHELL_TIMEOUT_MS: u64 = 30_000;

/// Maximum shell command output before truncation.
pub const MAX_SHELL_OUTPUT_CHARS: usize = 50_000;

// ---------------------------------------------------------------------------
// Web & content extraction
// ---------------------------------------------------------------------------

/// Safety cap for extracted web page text.
///
/// 120K chars ≈ 30K tokens. Generous so the agent sees full pages (product
/// lists, price trackers, deep reviews) without silent truncation. The
/// agent-level `context_pressure` nudge at 250K chars handles overall
/// context budget.
pub const MAX_EXTRACT_CHARS: usize = 120_000;

// ---------------------------------------------------------------------------
// Knowledge search
// ---------------------------------------------------------------------------

/// Default number of results returned by knowledge search.
pub const DEFAULT_KNOWLEDGE_SEARCH_LIMIT: usize = 10;

/// Default number of web search results.
pub const DEFAULT_WEB_SEARCH_RESULTS: usize = 5;

// ---------------------------------------------------------------------------
// Embeddings
// ---------------------------------------------------------------------------

/// Target chunk size for content splitting (characters).
pub const EMBEDDING_CHUNK_TARGET: usize = 2000;

/// Maximum code file size for embedding (100 KB).
pub const MAX_CODE_FILE_SIZE: u64 = 100 * 1024;

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// Maximum image dimension (width or height) before resizing.
pub const MAX_IMAGE_DIMENSION: u32 = 2048;

/// JPEG compression quality (0–100).
pub const JPEG_QUALITY: u8 = 85;

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

/// Maximum agent spawn nesting depth. Root = depth 0, child = depth 1,
/// depth >= 2 is dropped.
pub const MAX_SPAWN_DEPTH: u32 = 2;

// ---------------------------------------------------------------------------
// Browser
// ---------------------------------------------------------------------------

/// Default browser wait timeout (milliseconds) for element/navigation waits.
pub const DEFAULT_BROWSER_WAIT_MS: u64 = 1000;

/// Default CDP port for browser connections.
pub const DEFAULT_CDP_PORT: u16 = 9222;

/// Max DOM nodes to capture in accessibility snapshots.
pub const MAX_SNAPSHOT_NODES: usize = 500;

/// Max DOM tree depth for accessibility snapshots.
pub const MAX_SNAPSHOT_DEPTH: usize = 15;

// ---------------------------------------------------------------------------
// Event handler
// ---------------------------------------------------------------------------

/// Polling interval for idle session detection after a completion event.
pub const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Maximum idle polls before giving up (total wait = interval × polls).
pub const MAX_IDLE_POLLS: usize = 30;

/// Maximum retry attempts for continuation chat turns after transient errors.
pub const MAX_CONTINUATION_RETRIES: usize = 3;

/// Delay between retries for non-SessionBusy transient errors.
pub const CONTINUATION_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Maximum characters for system messages forwarded to Discord.
///
/// Discord v2 `text_display` components are capped at 4000 chars, and
/// `send_gateway_v2` prepends a `"**GHOST**\n\n"` header (~12 chars).
/// We leave margin to avoid hitting the limit.
pub const MAX_DISCORD_SYSTEM_MESSAGE_CHARS: usize = 3900;

// ---------------------------------------------------------------------------
// Web crawling
// ---------------------------------------------------------------------------

/// Default maximum crawl depth for reference imports.
pub const DEFAULT_CRAWL_MAX_DEPTH: usize = 3;

/// Default maximum pages to crawl per reference import.
pub const DEFAULT_CRAWL_MAX_PAGES: usize = 50;

/// Maximum characters allowed in a single YouTube transcript section.
pub const YOUTUBE_SECTION_MAX_CHARS: usize = 40_000;

/// Minimum characters required for a usable imported transcript.
pub const YOUTUBE_MIN_TRANSCRIPT_CHARS: usize = 500;

/// Minimum characters to keep a section after splitting.
pub const YOUTUBE_MIN_SECTION_CHARS: usize = 1_500;
