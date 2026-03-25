use super::document::{DoclingDocument, Provenance, Ref};

/// Generate markdown from a Docling document tree.
///
/// When `page_no` is `Some(n)`, only elements whose provenance includes page `n`
/// (or elements with no provenance at all) are emitted.
#[must_use]
pub fn generate_markdown(doc: &DoclingDocument, page_no: Option<u32>) -> String {
    let mut out = String::new();
    emit_refs(doc, &doc.body.children, page_no, &mut out);
    out
}

/// Parse a `$ref` like `"#/texts/0"` into `(collection, index)`.
fn parse_ref(ref_path: &str) -> Option<(&str, usize)> {
    let path = ref_path.strip_prefix("#/")?;
    let (collection, idx_str) = path.rsplit_once('/')?;
    let idx = idx_str.parse().ok()?;
    Some((collection, idx))
}

/// Whether an item matches the page filter.
fn matches_page(prov: &[Provenance], page_no: Option<u32>) -> bool {
    let Some(n) = page_no else {
        return true;
    };
    if prov.is_empty() {
        return true;
    }
    prov.iter().any(|p| p.page_no == n)
}

/// Walk a list of refs and append markdown to `out`.
fn emit_refs(doc: &DoclingDocument, refs: &[Ref], page_no: Option<u32>, out: &mut String) {
    for r in refs {
        let Some((collection, idx)) = parse_ref(&r.ref_path) else {
            continue;
        };

        match collection {
            "texts" => {
                let Some(item) = doc.texts.get(idx) else {
                    continue;
                };
                if !matches_page(&item.prov, page_no) {
                    continue;
                }
                emit_text_item(&item.label, &item.text, item.level, out);
                // Recurse into children if any.
                if !item.children.is_empty() {
                    emit_refs(doc, &item.children, page_no, out);
                }
            }
            "pictures" => {
                let Some(item) = doc.pictures.get(idx) else {
                    continue;
                };
                if !matches_page(&item.prov, page_no) {
                    continue;
                }
                out.push_str("<!-- image -->\n\n");
                // Recurse into children (e.g. captions stored as text refs).
                if !item.children.is_empty() {
                    emit_refs(doc, &item.children, page_no, out);
                }
            }
            "tables" => {
                let Some(item) = doc.tables.get(idx) else {
                    continue;
                };
                if !matches_page(&item.prov, page_no) {
                    continue;
                }
                out.push_str("<!-- table -->\n\n");
                if !item.children.is_empty() {
                    emit_refs(doc, &item.children, page_no, out);
                }
            }
            "groups" => {
                let Some(item) = doc.groups.get(idx) else {
                    continue;
                };
                if !matches_page(&item.prov, page_no) {
                    continue;
                }
                // Groups are just containers — recurse into their children.
                emit_refs(doc, &item.children, page_no, out);
            }
            _ => {
                // Unknown collection — skip silently.
            }
        }
    }
}

/// Emit a single text item as markdown based on its label.
fn emit_text_item(label: &str, text: &str, level: Option<u32>, out: &mut String) {
    match label {
        "title" => {
            out.push_str("# ");
            out.push_str(text);
            out.push_str("\n\n");
        }
        "section_header" => {
            let depth = level.unwrap_or(1) as usize;
            for _ in 0..depth {
                out.push('#');
            }
            out.push(' ');
            out.push_str(text);
            out.push_str("\n\n");
        }
        "list_item" => {
            out.push_str("- ");
            out.push_str(text);
            out.push('\n');
        }
        "code" => {
            out.push_str("```\n");
            out.push_str(text);
            out.push_str("\n```\n\n");
        }
        "formula" => {
            out.push('$');
            out.push_str(text);
            out.push_str("$\n\n");
        }
        // "text", "paragraph", and anything else — plain paragraph.
        _ => {
            out.push_str(text);
            out.push_str("\n\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docling::DoclingDocument;
    use crate::docling::document::{
        BodyNode, BoundingBox, GroupItem, PageInfo, PageSize, PictureItem, TextItem,
    };
    use std::collections::HashMap;

    /// Helper to build a Ref from a path string.
    fn mkref(path: &str) -> Ref {
        Ref {
            ref_path: path.to_string(),
        }
    }

    /// Helper to build provenance for a single page.
    fn prov(page: u32) -> Vec<Provenance> {
        vec![Provenance {
            page_no: page,
            bbox: None,
            charspan: vec![],
        }]
    }

    /// Helper to build a minimal pages map.
    fn pages(nums: &[u32]) -> HashMap<String, PageInfo> {
        nums.iter()
            .map(|&n| {
                (
                    n.to_string(),
                    PageInfo {
                        page_no: n,
                        size: Some(PageSize {
                            width: 100.0,
                            height: 100.0,
                        }),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn section_header_becomes_heading() {
        let doc = DoclingDocument {
            body: BodyNode {
                children: vec![mkref("#/texts/0")],
            },
            texts: vec![TextItem {
                label: "section_header".into(),
                text: "Hello".into(),
                prov: prov(1),
                children: vec![],
                level: Some(2),
            }],
            pictures: vec![],
            tables: vec![],
            groups: vec![],
            pages: pages(&[1]),
        };
        let md = generate_markdown(&doc, None);
        assert_eq!(md.trim(), "## Hello");
    }

    #[test]
    fn text_becomes_paragraph() {
        let doc = DoclingDocument {
            body: BodyNode {
                children: vec![mkref("#/texts/0")],
            },
            texts: vec![TextItem {
                label: "text".into(),
                text: "Some paragraph.".into(),
                prov: prov(1),
                children: vec![],
                level: None,
            }],
            pictures: vec![],
            tables: vec![],
            groups: vec![],
            pages: pages(&[1]),
        };
        let md = generate_markdown(&doc, None);
        assert_eq!(md.trim(), "Some paragraph.");
    }

    #[test]
    fn picture_becomes_placeholder() {
        let doc = DoclingDocument {
            body: BodyNode {
                children: vec![mkref("#/pictures/0")],
            },
            texts: vec![],
            pictures: vec![PictureItem {
                label: "picture".into(),
                prov: vec![Provenance {
                    page_no: 1,
                    bbox: Some(BoundingBox {
                        l: 0.0,
                        t: 100.0,
                        r: 100.0,
                        b: 0.0,
                    }),
                    charspan: vec![],
                }],
                children: vec![],
                captions: vec![],
                references: vec![],
                footnotes: vec![],
            }],
            tables: vec![],
            groups: vec![],
            pages: pages(&[1]),
        };
        let md = generate_markdown(&doc, None);
        assert_eq!(md.trim(), "<!-- image -->");
    }

    #[test]
    fn page_filter_only_returns_requested_page() {
        let doc = DoclingDocument {
            body: BodyNode {
                children: vec![mkref("#/texts/0"), mkref("#/texts/1")],
            },
            texts: vec![
                TextItem {
                    label: "text".into(),
                    text: "Page one.".into(),
                    prov: prov(1),
                    children: vec![],
                    level: None,
                },
                TextItem {
                    label: "text".into(),
                    text: "Page two.".into(),
                    prov: prov(2),
                    children: vec![],
                    level: None,
                },
            ],
            pictures: vec![],
            tables: vec![],
            groups: vec![],
            pages: pages(&[1, 2]),
        };
        let md = generate_markdown(&doc, Some(2));
        assert!(!md.contains("Page one"));
        assert!(md.contains("Page two"));
    }

    #[test]
    fn lotion_pdf_generates_some_output() {
        let json = include_str!("../../tests/fixtures/lotion_docling.json");
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let md = generate_markdown(&doc, None);
        // Should contain the garbled OCR text (this is what Docling extracted)
        assert!(md.contains("Fの"));
        assert!(!md.is_empty());
    }

    #[test]
    fn group_recurses_into_children() {
        let doc = DoclingDocument {
            body: BodyNode {
                children: vec![mkref("#/groups/0")],
            },
            texts: vec![TextItem {
                label: "text".into(),
                text: "Inside group.".into(),
                prov: prov(1),
                children: vec![],
                level: None,
            }],
            pictures: vec![],
            tables: vec![],
            groups: vec![GroupItem {
                label: "group".into(),
                children: vec![mkref("#/texts/0")],
                prov: prov(1),
            }],
            pages: pages(&[1]),
        };
        let md = generate_markdown(&doc, None);
        assert!(md.contains("Inside group."));
    }

    #[test]
    fn list_items() {
        let doc = DoclingDocument {
            body: BodyNode {
                children: vec![mkref("#/texts/0"), mkref("#/texts/1")],
            },
            texts: vec![
                TextItem {
                    label: "list_item".into(),
                    text: "First item".into(),
                    prov: prov(1),
                    children: vec![],
                    level: None,
                },
                TextItem {
                    label: "list_item".into(),
                    text: "Second item".into(),
                    prov: prov(1),
                    children: vec![],
                    level: None,
                },
            ],
            pictures: vec![],
            tables: vec![],
            groups: vec![],
            pages: pages(&[1]),
        };
        let md = generate_markdown(&doc, None);
        assert!(md.contains("- First item"));
        assert!(md.contains("- Second item"));
    }
}
