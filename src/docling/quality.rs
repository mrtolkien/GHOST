use super::document::*;

/// Per-page quality metrics.
#[derive(Debug)]
pub struct PageQuality {
    pub page_no: u32,
    pub text_chars: usize,
    pub text_item_count: usize,
    pub avg_text_length: f64,
    pub picture_area_ratio: f64,
    pub is_good: bool,
}

const MIN_TEXT_CHARS: usize = 50;
/// Pages where pictures cover more than 3% of the page area are considered
/// picture-heavy. Combined with low text, this indicates a scanned/image page
/// that needs vision re-extraction.
const MAX_PICTURE_AREA_RATIO: f64 = 0.03;
const MIN_AVG_TEXT_LENGTH: f64 = 5.0;

/// Assess quality for each page in the document.
pub fn assess_pages(doc: &DoclingDocument) -> Vec<PageQuality> {
    let mut results = Vec::new();

    for page_info in doc.pages.values() {
        let page_no = page_info.page_no;
        let page_area = page_info.size.as_ref().map_or(1.0, super::document::PageSize::area);

        let page_texts: Vec<&TextItem> = doc
            .texts
            .iter()
            .filter(|t| t.prov.iter().any(|p| p.page_no == page_no))
            .collect();

        let text_chars: usize = page_texts.iter().map(|t| t.text.len()).sum();
        let text_item_count = page_texts.len();
        let avg_text_length = if text_item_count > 0 {
            text_chars as f64 / text_item_count as f64
        } else {
            0.0
        };

        let picture_area: f64 = doc
            .pictures
            .iter()
            .filter(|p| p.prov.iter().any(|prov| prov.page_no == page_no))
            .filter_map(|p| p.prov.first())
            .filter_map(|prov| prov.bbox.as_ref())
            .map(super::document::BoundingBox::area)
            .sum();

        let picture_area_ratio = if page_area > 0.0 {
            picture_area / page_area
        } else {
            0.0
        };

        let is_good = !((text_chars < MIN_TEXT_CHARS
            && picture_area_ratio > MAX_PICTURE_AREA_RATIO)
            || (avg_text_length < MIN_AVG_TEXT_LENGTH && text_item_count > 0));

        results.push(PageQuality {
            page_no,
            text_chars,
            text_item_count,
            avg_text_length,
            picture_area_ratio,
            is_good,
        });
    }

    results.sort_by_key(|p| p.page_no);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lotion_pdf_flagged_as_bad() {
        let json = include_str!("../../tests/fixtures/lotion_docling.json");
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let pages = assess_pages(&doc);
        assert_eq!(pages.len(), 1);
        assert!(
            !pages[0].is_good,
            "lotion PDF page should be flagged as bad"
        );
        assert!(pages[0].text_chars < MIN_TEXT_CHARS);
        assert!(pages[0].picture_area_ratio > MAX_PICTURE_AREA_RATIO);
    }

    #[test]
    fn good_page_with_enough_text() {
        let json = r#"{
            "body": {"children": []},
            "texts": [
                {"label": "text", "text": "This is a paragraph with plenty of text content that exceeds the minimum threshold easily.", "prov": [{"page_no": 1}], "children": []}
            ],
            "pictures": [],
            "tables": [],
            "groups": [],
            "pages": {"1": {"page_no": 1, "size": {"width": 612, "height": 792}}}
        }"#;
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let pages = assess_pages(&doc);
        assert_eq!(pages.len(), 1);
        assert!(pages[0].is_good);
    }

    #[test]
    fn empty_page_flagged_as_bad() {
        let json = r#"{
            "body": {"children": []},
            "texts": [],
            "pictures": [
                {"label": "picture", "prov": [{"page_no": 1, "bbox": {"l": 0, "t": 792, "r": 612, "b": 0}}], "children": []}
            ],
            "tables": [],
            "groups": [],
            "pages": {"1": {"page_no": 1, "size": {"width": 612, "height": 792}}}
        }"#;
        let doc: DoclingDocument = serde_json::from_str(json).unwrap();
        let pages = assess_pages(&doc);
        assert_eq!(pages.len(), 1);
        assert!(!pages[0].is_good);
    }
}
