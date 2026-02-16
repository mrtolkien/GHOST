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
const ROW_HEIGHT: f32 = 36.0;
const CELL_PAD_X: f32 = 14.0;
const CELL_PAD_Y_TOP: f32 = 0.0;
const CORNER_RADIUS: f32 = 10.0;
const HEADER_ACCENT_HEIGHT: f32 = 3.0;

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

    let col_px: Vec<f32> = col_chars
        .iter()
        .map(|&n| n as f32 * CHAR_WIDTH + 2.0 * CELL_PAD_X)
        .collect();

    let total_w = col_px.iter().sum::<f32>().ceil();
    let total_h = (rows.len() as f32 * ROW_HEIGHT + HEADER_ACCENT_HEIGHT).ceil();

    let svg = build_svg(&rows, &col_px, col_count, total_w, total_h);

    match rasterize(&svg) {
        Ok(png) => Some(png),
        Err(e) => {
            warn!("Table image render failed: {e}");
            None
        }
    }
}

fn build_svg(rows: &[Vec<String>], col_px: &[f32], col_count: usize, w: f32, h: f32) -> String {
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
    let _ = write!(
        s,
        r#"<rect width="{w}" height="{ROW_HEIGHT}" fill="{HEADER_BG}"/>"#,
    );

    let accent_y = ROW_HEIGHT;
    let _ = write!(
        s,
        r#"<rect y="{accent_y}" width="{w}" height="{HEADER_ACCENT_HEIGHT}" fill="{HEADER_ACCENT}"/>"#,
    );

    let data_top = ROW_HEIGHT + HEADER_ACCENT_HEIGHT;
    for i in 1..rows.len() {
        let fill = if i % 2 == 0 { ZEBRA_ODD } else { ZEBRA_EVEN };
        let ry = data_top + (i - 1) as f32 * ROW_HEIGHT;
        let _ = write!(
            s,
            r#"<rect y="{ry}" width="{w}" height="{ROW_HEIGHT}" fill="{fill}"/>"#,
        );
    }

    let mut x = 0.0;
    for &cw in col_px.iter().take(col_count - 1) {
        x += cw;
        let _ = write!(
            s,
            r#"<line x1="{x}" y1="0" x2="{x}" y2="{h}" stroke="{BORDER_COLOR}" stroke-width="0.5" opacity="0.5"/>"#,
        );
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let is_header = row_idx == 0;
        let fill = if is_header { HEADER_TEXT } else { TEXT_COLOR };
        let weight = if is_header { "600" } else { "400" };
        let letter_spacing = if is_header {
            " letter-spacing=\"0.3\""
        } else {
            ""
        };

        let row_top = if is_header {
            0.0
        } else {
            ROW_HEIGHT + HEADER_ACCENT_HEIGHT + (row_idx - 1) as f32 * ROW_HEIGHT
        };
        let baseline_y = row_top + CELL_PAD_Y_TOP + ROW_HEIGHT * 0.62;

        let mut col_x = 0.0_f32;
        for (col_idx, cell) in row.iter().enumerate() {
            if col_idx >= col_count {
                break;
            }
            let tx = col_x + CELL_PAD_X;
            let spans = parse_inline(cell);
            let _ = write!(
                s,
                r#"<text x="{tx}" y="{baseline_y}" font-family="{FONT_FAMILY}" font-size="{FONT_SIZE}" fill="{fill}" font-weight="{weight}"{letter_spacing}>"#,
            );
            for span in &spans {
                let escaped = xml_escape(&span.text);
                match span.style {
                    SpanStyle::Normal => {
                        let _ = write!(s, "{escaped}");
                    }
                    SpanStyle::Bold => {
                        let _ = write!(s, r#"<tspan font-weight="700">{escaped}</tspan>"#);
                    }
                    SpanStyle::Italic => {
                        let _ = write!(s, r#"<tspan font-style="italic">{escaped}</tspan>"#);
                    }
                    SpanStyle::Code => {
                        let _ = write!(s, r#"<tspan font-family="monospace">{escaped}</tspan>"#,);
                    }
                }
            }
            let _ = write!(s, "</text>");
            col_x += col_px[col_idx];
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
}
