use std::collections::HashMap;

use serde::Deserialize;

/// Top-level Docling document structure.
///
/// We intentionally do NOT use `#[serde(deny_unknown_fields)]` anywhere in this
/// module. Docling's JSON contains many fields we don't need (e.g. `schema_name`,
/// `version`, `origin`, `furniture`, `self_ref`, `content_layer`, etc.), and new
/// Docling versions may add more. Unknown fields are silently ignored.
#[derive(Debug, Deserialize)]
pub struct DoclingDocument {
    #[serde(default)]
    pub body: BodyNode,

    #[serde(default)]
    pub texts: Vec<TextItem>,

    #[serde(default)]
    pub pictures: Vec<PictureItem>,

    #[serde(default)]
    pub tables: Vec<TableItem>,

    #[serde(default)]
    pub groups: Vec<GroupItem>,

    #[serde(default)]
    pub pages: HashMap<String, PageInfo>,
}

/// The document body — a tree root with children refs.
#[derive(Debug, Default, Deserialize)]
pub struct BodyNode {
    #[serde(default)]
    pub children: Vec<Ref>,
}

/// A JSON `$ref` pointer like `{"$ref": "#/texts/0"}`.
#[derive(Debug, Deserialize)]
pub struct Ref {
    #[serde(rename = "$ref")]
    pub ref_path: String,
}

/// A text element extracted from the document.
#[derive(Debug, Deserialize)]
pub struct TextItem {
    #[serde(default)]
    pub label: String,

    #[serde(default)]
    pub text: String,

    #[serde(default)]
    pub prov: Vec<Provenance>,

    #[serde(default)]
    pub children: Vec<Ref>,

    /// Heading level (present for `section_header` labels).
    pub level: Option<u32>,
}

/// A picture element.
#[derive(Debug, Deserialize)]
pub struct PictureItem {
    #[serde(default)]
    pub label: String,

    #[serde(default)]
    pub prov: Vec<Provenance>,

    #[serde(default)]
    pub children: Vec<Ref>,

    #[serde(default)]
    pub captions: Vec<Ref>,

    #[serde(default)]
    pub references: Vec<Ref>,

    #[serde(default)]
    pub footnotes: Vec<Ref>,
}

/// A table element. The grid data is stored as opaque JSON for now since we
/// don't yet need to interpret table structure.
#[derive(Debug, Deserialize)]
pub struct TableItem {
    #[serde(default)]
    pub label: String,

    #[serde(default)]
    pub prov: Vec<Provenance>,

    #[serde(default)]
    pub children: Vec<Ref>,

    /// Raw table grid data — kept as opaque JSON until we need it.
    pub data: Option<serde_json::Value>,
}

/// A logical group of elements.
#[derive(Debug, Deserialize)]
pub struct GroupItem {
    #[serde(default)]
    pub label: String,

    #[serde(default)]
    pub children: Vec<Ref>,

    #[serde(default)]
    pub prov: Vec<Provenance>,
}

/// Provenance: where an element was found on a page.
#[derive(Debug, Deserialize)]
pub struct Provenance {
    pub page_no: u32,

    pub bbox: Option<BoundingBox>,

    #[serde(default)]
    pub charspan: Vec<u32>,
}

/// Axis-aligned bounding box (in PDF points).
#[derive(Debug, Deserialize)]
pub struct BoundingBox {
    pub l: f64,
    pub t: f64,
    pub r: f64,
    pub b: f64,
}

impl BoundingBox {
    /// Area of the bounding box.
    #[must_use]
    pub fn area(&self) -> f64 {
        (self.r - self.l).abs() * (self.t - self.b).abs()
    }
}

/// Page metadata.
#[derive(Debug, Deserialize)]
pub struct PageInfo {
    pub size: Option<PageSize>,
    pub page_no: u32,
}

/// Physical page dimensions (in PDF points).
#[derive(Debug, Deserialize)]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

impl PageSize {
    /// Area of the page.
    #[must_use]
    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_lotion_pdf() {
        let json = include_str!("../../tests/fixtures/lotion_docling.json");
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        assert_eq!(doc.texts.len(), 4);
        assert_eq!(doc.pictures.len(), 5);
        assert_eq!(doc.tables.len(), 0);
        assert_eq!(doc.groups.len(), 1);
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.body.children.len(), 8);
        // Verify ref parsing
        assert_eq!(doc.body.children[0].ref_path, "#/pictures/0");
        // Verify text content
        assert_eq!(doc.texts[2].text, "Fの");
        assert_eq!(doc.texts[2].label, "section_header");
        assert_eq!(doc.texts[2].level, Some(1));
        // Verify provenance
        assert_eq!(doc.texts[0].prov[0].page_no, 1);
        assert!(doc.texts[0].prov[0].bbox.is_some());
    }
}
