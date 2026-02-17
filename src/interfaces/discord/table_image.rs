/// SVG-based table-to-PNG renderer for Discord messages.
///
/// Builds an SVG from parsed markdown table rows and rasterizes it via resvg.
/// Styled with a modern dark theme for seamless inline display in Discord.
use std::fmt::Write;
use std::sync::LazyLock;

use resvg::tiny_skia;
use resvg::usvg;
use tracing::warn;

// ---------------------------------------------------------------------------
// Render scale: generate a 2x SVG for crisp images on HiDPI / Discord scaling
// ---------------------------------------------------------------------------
const SCALE: f32 = 2.0;

// ---------------------------------------------------------------------------
// Layout (logical pixels — multiplied by SCALE at rasterization)
// ---------------------------------------------------------------------------
const FONT_SIZE: f32 = 14.0;
/// Average character width for a proportional sans-serif at FONT_SIZE.
/// Slightly overestimated to prevent text overflow.
const CHAR_WIDTH: f32 = 8.4;
const MIN_ROW_HEIGHT: f32 = 36.0;
const LINE_HEIGHT: f32 = 18.0;
const CELL_PAD_X: f32 = 14.0;
const CELL_PAD_Y: f32 = 8.0;
const CORNER_RADIUS: f32 = 10.0;
const HEADER_ACCENT_HEIGHT: f32 = 3.0;
const MAX_COL_CHARS: usize = 60;

// ---------------------------------------------------------------------------
// Color palette — refined Discord dark theme
// ---------------------------------------------------------------------------
const BG_COLOR: &str = "#2B2D31";
const HEADER_BG: &str = "#1E1F22";
const HEADER_ACCENT: &str = "#5865F2";
const ZEBRA_EVEN: &str = "#2B2D31";
const ZEBRA_ODD: &str = "#2E3035";
const TEXT_COLOR: &str = "#D2D5D9";
const HEADER_TEXT: &str = "#FFFFFF";
const BORDER_COLOR: &str = "#3B3D44";

// ---------------------------------------------------------------------------
// Font stack — proportional for readability, bold actually renders
// ---------------------------------------------------------------------------
const FONT_FAMILY: &str = "'Inter', 'Segoe UI', 'Helvetica Neue', 'Arial', 'Noto Sans', sans-serif";

static SVG_OPTIONS: LazyLock<usvg::Options> = LazyLock::new(|| {
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    opt
});

/// Eagerly initialize the system font database.
///
/// The underlying `LazyLock` scans every font file on the system, which can
/// block for seconds on large font collections.  Calling this at startup
/// (from a blocking context) avoids stalling the tokio runtime on the first
/// table render.
pub(super) fn init_fonts() {
    LazyLock::force(&SVG_OPTIONS);
}

/// Render a parsed markdown table to PNG bytes.
///
/// `raw_lines` are the original markdown table lines (header, separator,
/// data). The separator row is stripped automatically.
/// Returns `None` if rendering fails (missing fonts, empty table, etc.).
pub(super) fn render_table_png(raw_lines: &[String]) -> Option<Vec<u8>> {
    let rows: Vec<Vec<String>> = raw_lines
        .iter()
        .filter(|l| !is_separator_line(l))
        .map(|l| parse_cells(l))
        .collect();

    if rows.is_empty() {
        return None;
    }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return None;
    }

    // Column widths in visible characters (markdown markers excluded).
    // Header cells get a small bonus to account for bold being slightly wider.
    let mut col_chars = vec![3usize; col_count];
    for (row_idx, row) in rows.iter().enumerate() {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                let len = visible_len(cell);
                let effective = if row_idx == 0 {
                    len + (len / 8).max(1)
                } else {
                    len
                };
                col_chars[i] = col_chars[i].max(effective);
            }
        }
    }

    let col_chars: Vec<usize> = col_chars
        .into_iter()
        .map(|n| n.min(MAX_COL_CHARS))
        .collect();

    let col_px: Vec<f32> = col_chars
        .iter()
        .map(|&n| n as f32 * CHAR_WIDTH + 2.0 * CELL_PAD_X)
        .collect();

    let total_w = col_px.iter().sum::<f32>().ceil();
    let wrapped_rows = wrap_rows(&rows, &col_chars, col_count);
    let row_heights = compute_row_heights(&wrapped_rows);
    let total_h = (row_heights.iter().sum::<f32>() + HEADER_ACCENT_HEIGHT).ceil();

    let svg = build_svg(
        &wrapped_rows,
        &row_heights,
        &col_px,
        col_count,
        total_w,
        total_h,
    );

    match rasterize(&svg) {
        Ok(png) => Some(png),
        Err(e) => {
            warn!("Table image render failed: {e}");
            None
        }
    }
}

fn build_svg(
    rows: &[Vec<Vec<Vec<StyledSpan>>>],
    row_heights: &[f32],
    col_px: &[f32],
    col_count: usize,
    w: f32,
    h: f32,
) -> String {
    let pw = (w * SCALE).ceil();
    let ph = (h * SCALE).ceil();

    let mut s = String::with_capacity(4096);

    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pw}" height="{ph}" viewBox="0 0 {w} {h}">"#,
    );

    let _ = write!(
        s,
        r#"<defs><clipPath id="table-clip"><rect width="{w}" height="{h}" rx="{CORNER_RADIUS}"/></clipPath></defs>"#,
    );

    let _ = write!(s, r#"<g clip-path="url(#table-clip)">"#);
    let _ = write!(s, r#"<rect width="{w}" height="{h}" fill="{BG_COLOR}"/>"#);
    let header_h = row_heights.first().copied().unwrap_or(MIN_ROW_HEIGHT);
    let _ = write!(
        s,
        r#"<rect width="{w}" height="{header_h}" fill="{HEADER_BG}"/>"#,
    );

    let accent_y = header_h;
    let _ = write!(
        s,
        r#"<rect y="{accent_y}" width="{w}" height="{HEADER_ACCENT_HEIGHT}" fill="{HEADER_ACCENT}"/>"#,
    );

    let data_top = header_h + HEADER_ACCENT_HEIGHT;
    let mut row_y = data_top;
    for i in 1..rows.len() {
        let fill = if i % 2 == 0 { ZEBRA_ODD } else { ZEBRA_EVEN };
        let ry = row_y;
        let row_h = row_heights.get(i).copied().unwrap_or(MIN_ROW_HEIGHT);
        let _ = write!(
            s,
            r#"<rect y="{ry}" width="{w}" height="{row_h}" fill="{fill}"/>"#,
        );
        row_y += row_h;
    }

    let mut x = 0.0;
    for &cw in col_px.iter().take(col_count - 1) {
        x += cw;
        let _ = write!(
            s,
            r#"<line x1="{x}" y1="0" x2="{x}" y2="{h}" stroke="{BORDER_COLOR}" stroke-width="0.5" opacity="0.5"/>"#,
        );
    }

    let mut row_top = 0.0_f32;
    for (row_idx, row) in rows.iter().enumerate() {
        let is_header = row_idx == 0;
        let fill = if is_header { HEADER_TEXT } else { TEXT_COLOR };
        let weight = if is_header { "600" } else { "400" };
        let letter_spacing = if is_header {
            " letter-spacing=\"0.3\""
        } else {
            ""
        };

        let row_h = row_heights.get(row_idx).copied().unwrap_or(MIN_ROW_HEIGHT);
        let mut col_x = 0.0_f32;
        for (col_idx, col_w) in col_px.iter().enumerate().take(col_count) {
            let tx = col_x + CELL_PAD_X;
            let start_y = row_top + CELL_PAD_Y + FONT_SIZE;
            if let Some(cell) = row.get(col_idx) {
                for (line_idx, line) in cell.iter().enumerate() {
                    let baseline_y = start_y + line_idx as f32 * LINE_HEIGHT;
                    let _ = write!(
                        s,
                        r#"<text x="{tx}" y="{baseline_y}" font-family="{FONT_FAMILY}" font-size="{FONT_SIZE}" fill="{fill}" font-weight="{weight}"{letter_spacing}>"#,
                    );
                    for span in line {
                        let escaped = xml_escape(&span.text);
                        match span.style {
                            SpanStyle::Normal => {
                                let _ = write!(s, "{escaped}");
                            }
                            SpanStyle::Bold => {
                                let _ = write!(s, r#"<tspan font-weight="700">{escaped}</tspan>"#);
                            }
                            SpanStyle::Italic => {
                                let _ =
                                    write!(s, r#"<tspan font-style="italic">{escaped}</tspan>"#);
                            }
                            SpanStyle::Code => {
                                let _ = write!(
                                    s,
                                    r#"<tspan font-family="'JetBrains Mono', 'Fira Code', 'Source Code Pro', 'DejaVu Sans Mono', 'Liberation Mono', 'Courier New', monospace">{escaped}</tspan>"#,
                                );
                            }
                        }
                    }
                    let _ = write!(s, "</text>");
                }
            }
            col_x += col_w;
        }
        row_top += row_h;
        if is_header {
            row_top += HEADER_ACCENT_HEIGHT;
        }
    }

    let _ = write!(
        s,
        r#"</g><rect width="{w}" height="{h}" rx="{CORNER_RADIUS}" fill="none" stroke="{BORDER_COLOR}" stroke-width="1"/>"#,
    );

    s.push_str("</svg>");
    s
}

fn rasterize(svg_str: &str) -> Result<Vec<u8>, String> {
    let tree = usvg::Tree::from_data(svg_str.as_bytes(), &SVG_OPTIONS)
        .map_err(|e| format!("SVG parse: {e}"))?;

    let size = tree.size().to_int_size();
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("pixmap allocation failed")?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    pixmap.encode_png().map_err(|e| format!("PNG encode: {e}"))
}

// ---------------------------------------------------------------------------
// Inline markdown -> styled spans
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum SpanStyle {
    Normal,
    Bold,
    Italic,
    Code,
}

#[derive(Debug)]
struct StyledSpan {
    text: String,
    style: SpanStyle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TokenKind {
    Word,
    Space,
    Newline,
}

#[derive(Debug)]
struct StyledToken {
    text: String,
    style: SpanStyle,
    kind: TokenKind,
}

/// Parse inline markdown (`**bold**`, `*italic*`, `` `code` ``) into styled
/// spans.
fn parse_inline(input: &str) -> Vec<StyledSpan> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            '`' => {
                flush(&mut spans, &mut buf, SpanStyle::Normal);
                i += 1;
                while i < len && chars[i] != '`' {
                    buf.push(chars[i]);
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                flush(&mut spans, &mut buf, SpanStyle::Code);
            }
            '*' if i + 1 < len && chars[i + 1] == '*' => {
                flush(&mut spans, &mut buf, SpanStyle::Normal);
                i += 2;
                while i < len {
                    if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
                        i += 2;
                        break;
                    }
                    buf.push(chars[i]);
                    i += 1;
                }
                flush(&mut spans, &mut buf, SpanStyle::Bold);
            }
            '*' => {
                flush(&mut spans, &mut buf, SpanStyle::Normal);
                i += 1;
                while i < len && chars[i] != '*' {
                    buf.push(chars[i]);
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                flush(&mut spans, &mut buf, SpanStyle::Italic);
            }
            ch => {
                buf.push(ch);
                i += 1;
            }
        }
    }

    flush(&mut spans, &mut buf, SpanStyle::Normal);
    spans
}

fn wrap_rows(
    rows: &[Vec<String>],
    col_chars: &[usize],
    col_count: usize,
) -> Vec<Vec<Vec<Vec<StyledSpan>>>> {
    rows.iter()
        .map(|row| {
            (0..col_count)
                .map(|col_idx| {
                    let text = row.get(col_idx).map_or("", String::as_str);
                    wrap_inline_spans(&parse_inline(text), col_chars[col_idx])
                })
                .collect()
        })
        .collect()
}

fn compute_row_heights(rows: &[Vec<Vec<Vec<StyledSpan>>>]) -> Vec<f32> {
    rows.iter()
        .map(|row| {
            let max_lines = row.iter().map(Vec::len).max().unwrap_or(1).max(1) as f32;
            (CELL_PAD_Y * 2.0 + max_lines * LINE_HEIGHT).max(MIN_ROW_HEIGHT)
        })
        .collect()
}

fn wrap_inline_spans(spans: &[StyledSpan], max_chars: usize) -> Vec<Vec<StyledSpan>> {
    let max_chars = max_chars.max(1);
    let tokens = spans_to_tokens(spans);
    let mut lines: Vec<Vec<StyledSpan>> = vec![Vec::new()];
    let mut current_len = 0usize;

    for token in tokens {
        match token.kind {
            TokenKind::Newline => {
                lines.push(Vec::new());
                current_len = 0;
            }
            TokenKind::Space => {
                if current_len == 0 {
                    continue;
                }
                let token_len = token.text.chars().count();
                if current_len + token_len > max_chars {
                    lines.push(Vec::new());
                    current_len = 0;
                    continue;
                }
                push_span(&mut lines, token.style, &token.text);
                current_len += token_len;
            }
            TokenKind::Word => {
                let mut rest = token.text.chars().collect::<Vec<char>>();
                while !rest.is_empty() {
                    if current_len >= max_chars {
                        lines.push(Vec::new());
                        current_len = 0;
                    }
                    let available = max_chars - current_len;
                    if available == 0 {
                        continue;
                    }
                    let take = available.min(rest.len());
                    let chunk: String = rest.drain(..take).collect();
                    push_span(&mut lines, token.style, &chunk);
                    current_len += take;
                    if !rest.is_empty() {
                        lines.push(Vec::new());
                        current_len = 0;
                    }
                }
            }
        }
    }

    if lines.is_empty() {
        return vec![Vec::new()];
    }
    lines
}

fn spans_to_tokens(spans: &[StyledSpan]) -> Vec<StyledToken> {
    let mut tokens = Vec::new();
    for span in spans {
        let mut buf = String::new();
        let mut kind: Option<TokenKind> = None;
        for ch in span.text.chars() {
            if ch == '\n' {
                flush_token(&mut tokens, &mut buf, kind, span.style);
                kind = None;
                tokens.push(StyledToken {
                    text: "\n".to_string(),
                    style: span.style,
                    kind: TokenKind::Newline,
                });
                continue;
            }
            let next_kind = if ch.is_whitespace() {
                TokenKind::Space
            } else {
                TokenKind::Word
            };
            if kind != Some(next_kind) {
                flush_token(&mut tokens, &mut buf, kind, span.style);
                kind = Some(next_kind);
            }
            buf.push(ch);
        }
        flush_token(&mut tokens, &mut buf, kind, span.style);
    }
    tokens
}

fn flush_token(
    tokens: &mut Vec<StyledToken>,
    buf: &mut String,
    kind: Option<TokenKind>,
    style: SpanStyle,
) {
    if let Some(kind) = kind
        && !buf.is_empty()
    {
        tokens.push(StyledToken {
            text: std::mem::take(buf),
            style,
            kind,
        });
    }
}

fn push_span(lines: &mut [Vec<StyledSpan>], style: SpanStyle, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last_line) = lines.last_mut() {
        if let Some(last_span) = last_line.last_mut()
            && last_span.style == style
        {
            last_span.text.push_str(text);
            return;
        }
        last_line.push(StyledSpan {
            text: text.to_string(),
            style,
        });
    }
}

fn flush(spans: &mut Vec<StyledSpan>, buf: &mut String, style: SpanStyle) {
    if !buf.is_empty() {
        spans.push(StyledSpan {
            text: std::mem::take(buf),
            style,
        });
    }
}

/// Count visible characters (excluding markdown markers).
fn visible_len(input: &str) -> usize {
    parse_inline(input)
        .iter()
        .map(|s| s.text.chars().count())
        .sum()
}

fn parse_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_separator_line(line: &str) -> bool {
    let trimmed = line.trim();
    if !(trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() > 2) {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    inner
        .split('|')
        .all(|cell| cell.trim().chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_returns_none() {
        assert!(render_table_png(&[]).is_none());
    }

    #[test]
    fn separator_only_returns_none() {
        let lines = vec!["|---|---|".to_string()];
        assert!(render_table_png(&lines).is_none());
    }

    #[test]
    fn simple_table_renders_png() {
        let lines = vec![
            "| Name | Value |".to_string(),
            "|------|-------|".to_string(),
            "| A    | 1     |".to_string(),
            "| B    | 2     |".to_string(),
        ];
        let png = render_table_png(&lines);
        assert!(
            png.is_some(),
            "PNG rendering failed — system fonts may be missing"
        );
        let bytes = png.unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");
    }

    #[test]
    fn xml_special_chars_escaped() {
        assert_eq!(xml_escape("a<b>&\"c"), "a&lt;b&gt;&amp;&quot;c");
    }

    #[test]
    fn parse_cells_strips_padding() {
        let cells = parse_cells("| Hello | World |");
        assert_eq!(cells, vec!["Hello", "World"]);
    }

    #[test]
    fn separator_detection() {
        assert!(is_separator_line("|---|---|"));
        assert!(is_separator_line("| --- | :---: |"));
        assert!(!is_separator_line("| A | B |"));
    }

    #[test]
    fn parse_inline_plain_text() {
        let spans = parse_inline("hello world");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "hello world");
        assert_eq!(spans[0].style, SpanStyle::Normal);
    }

    #[test]
    fn parse_inline_bold() {
        let spans = parse_inline("before **bold** after");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].text, "before ");
        assert_eq!(spans[0].style, SpanStyle::Normal);
        assert_eq!(spans[1].text, "bold");
        assert_eq!(spans[1].style, SpanStyle::Bold);
        assert_eq!(spans[2].text, " after");
        assert_eq!(spans[2].style, SpanStyle::Normal);
    }

    #[test]
    fn parse_inline_italic() {
        let spans = parse_inline("some *italic* text");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].text, "italic");
        assert_eq!(spans[1].style, SpanStyle::Italic);
    }

    #[test]
    fn parse_inline_code() {
        let spans = parse_inline("run `cargo test` now");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1].text, "cargo test");
        assert_eq!(spans[1].style, SpanStyle::Code);
    }

    #[test]
    fn parse_inline_mixed() {
        let spans = parse_inline("**bold** and `code`");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].style, SpanStyle::Bold);
        assert_eq!(spans[1].style, SpanStyle::Normal);
        assert_eq!(spans[2].style, SpanStyle::Code);
    }

    #[test]
    fn visible_len_strips_markers() {
        assert_eq!(visible_len("**bold**"), 4);
        assert_eq!(visible_len("plain **bold** more"), 15);
        assert_eq!(visible_len("`code`"), 4);
        assert_eq!(visible_len("no markers"), 10);
    }

    #[test]
    fn wrap_inline_wraps_long_words() {
        let lines = wrap_inline_spans(&parse_inline("abcdefghij"), 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0][0].text, "abcd");
        assert_eq!(lines[1][0].text, "efgh");
        assert_eq!(lines[2][0].text, "ij");
    }

    #[test]
    fn wrap_inline_preserves_styles() {
        let lines = wrap_inline_spans(&parse_inline("**bold** tail"), 6);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].style, SpanStyle::Bold);
        assert_eq!(lines[1][0].style, SpanStyle::Normal);
    }

    #[test]
    #[ignore = "writes preview file to /tmp — run manually"]
    fn write_preview_png() {
        let lines = vec![
            "| Kind | Identifier | Summary |".to_string(),
            "|------|------------|---------|".to_string(),
            "| **Reference** | `42a525e0` 3d-printers | Enclosed 3D printers for *home use* |"
                .to_string(),
            "| **Web fetch** | https://docs.surrealdb.com/ | Top three **SurrealDB** features |"
                .to_string(),
            "| **Note** | ping test | OPERATOR sent a *ping* test. |".to_string(),
        ];
        let png = render_table_png(&lines).expect("render failed");
        std::fs::write("/tmp/ghost-table-preview.png", &png).expect("write failed");
        eprintln!("Preview written to /tmp/ghost-table-preview.png");
    }

    #[test]
    #[ignore = "writes long-table preview file to /tmp — run manually"]
    fn write_long_text_preview_png() {
        let lines = vec![
            "| Skill | When to Use |".to_string(),
            "|-------|-------------|".to_string(),
            "| **cron-job-author** | When the OPERATOR wants to create **scheduled, recurring tasks** that run automatically. Examples: daily summaries, hourly checks, weekly reports. These live in `jobs/*.md` and run on cron schedules. |".to_string(),
            "| **note-writer** | When you need to **persist important information** to the knowledge base. Examples: diary entries, identity notes, summaries of decisions, research findings that should be remembered long-term. |".to_string(),
            "| **reference-researcher** | When you need to do **deep research** on a topic and build a high-quality reference document. This guides systematic web searches, source evaluation, and creating authoritative references that can be cited later. |".to_string(),
            "| **skill-creator** | When the OPERATOR wants to **create a new skill** or **update an existing one**. This provides guidance on structuring effective skills, writing clear instructions, and making them reusable. |".to_string(),
            "| **web-search** | When doing any **web research or fetching**. This covers advanced search strategies, choosing between search/fetch modes, result curation, and building effective research workflows. |".to_string(),
        ];
        let png = render_table_png(&lines).expect("render failed");
        std::fs::write("/tmp/ghost-table-preview-long.png", &png).expect("write failed");
        eprintln!("Preview written to /tmp/ghost-table-preview-long.png");
    }
}
