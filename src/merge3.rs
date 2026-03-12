use std::ops::Range;

use similar::{ChangeTag, DiffTag, TextDiff};

/// Result of a three-way merge operation.
#[derive(Debug, PartialEq)]
pub enum MergeResult {
    /// All changes merged without conflict.
    Clean(String),
    /// Conflicting changes — output contains conflict markers.
    Conflict(String),
}

impl MergeResult {
    /// Returns the merged content (with or without conflict markers).
    pub fn content(&self) -> &str {
        match self {
            MergeResult::Clean(s) | MergeResult::Conflict(s) => s,
        }
    }

    /// Returns `true` if the merge completed without conflicts.
    pub fn is_clean(&self) -> bool {
        matches!(self, MergeResult::Clean(_))
    }
}

/// A region where one side changed relative to the base.
#[derive(Debug, Clone)]
struct EditRegion {
    /// Range of lines in the base that this edit covers.
    base_range: Range<usize>,
    /// The replacement lines (empty for pure deletions).
    new_lines: Vec<String>,
}

/// Performs a line-based three-way merge.
///
/// Given `base` (common ancestor), `ours` (workspace version), and `theirs`
/// (new bundle version), produces a merged result. Non-overlapping edits from
/// both sides are combined. Overlapping edits with identical content are
/// deduplicated. Overlapping edits with different content produce conflict
/// markers.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> MergeResult {
    // Trivial cases
    if ours == theirs {
        return MergeResult::Clean(ours.to_string());
    }
    if base == ours {
        return MergeResult::Clean(theirs.to_string());
    }
    if base == theirs {
        return MergeResult::Clean(ours.to_string());
    }

    let base_lines: Vec<&str> = split_lines(base);
    let our_edits = diff_regions(base, ours);
    let their_edits = diff_regions(base, theirs);

    merge_edits(&base_lines, &our_edits, &their_edits)
}

/// Splits text into lines, preserving trailing newlines for each line.
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let diff = TextDiff::from_lines(text, "");
    diff.iter_all_changes()
        .filter(|c| c.tag() == ChangeTag::Delete)
        .map(|c| c.value())
        .collect()
}

/// Extracts edit regions from a diff between `base` and `modified`.
///
/// Each region describes a range of base lines and what they should be replaced
/// with.
fn diff_regions(base: &str, modified: &str) -> Vec<EditRegion> {
    let diff = TextDiff::from_lines(base, modified);
    let mut regions = Vec::new();

    for op in diff.ops() {
        let (tag, old_range, new_range) = op.as_tag_tuple();
        match tag {
            DiffTag::Equal => {}
            DiffTag::Delete => {
                regions.push(EditRegion {
                    base_range: old_range,
                    new_lines: Vec::new(),
                });
            }
            DiffTag::Insert | DiffTag::Replace => {
                let modified_lines: Vec<&str> = split_lines(modified);
                let new_lines: Vec<String> =
                    modified_lines[new_range].iter().map(|s| (*s).to_string()).collect();
                regions.push(EditRegion {
                    base_range: old_range,
                    new_lines,
                });
            }
        }
    }

    regions
}

/// Merges edit regions from both sides against the base lines.
///
/// Walks through all edits sorted by base position. Non-overlapping edits are
/// applied directly. Overlapping edits with identical replacement content are
/// deduplicated. Overlapping edits with different content produce conflict
/// markers.
fn merge_edits(
    base_lines: &[&str],
    our_edits: &[EditRegion],
    their_edits: &[EditRegion],
) -> MergeResult {
    let mut result = String::new();
    let mut has_conflict = false;
    let mut base_pos: usize = 0;

    let mut oi = 0; // index into our_edits
    let mut ti = 0; // index into their_edits

    while oi < our_edits.len() || ti < their_edits.len() {
        let our_edit = our_edits.get(oi);
        let their_edit = their_edits.get(ti);

        match (our_edit, their_edit) {
            (Some(ours), Some(theirs)) => {
                if regions_overlap(ours, theirs) {
                    // Overlapping regions
                    let merged_start = ours.base_range.start.min(theirs.base_range.start);
                    let merged_end = ours.base_range.end.max(theirs.base_range.end);
                    append_base_lines(&mut result, base_lines, base_pos, merged_start);
                    if ours.new_lines == theirs.new_lines {
                        // Identical changes — apply once
                        append_edit(&mut result, ours);
                    } else {
                        // Conflict
                        has_conflict = true;
                        emit_conflict(&mut result, ours, theirs);
                    }
                    base_pos = merged_end;
                    oi += 1;
                    ti += 1;
                } else if ours.base_range.start <= theirs.base_range.start {
                    // Ours comes first, no overlap
                    append_base_lines(
                        &mut result,
                        base_lines,
                        base_pos,
                        ours.base_range.start,
                    );
                    append_edit(&mut result, ours);
                    base_pos = ours.base_range.end;
                    oi += 1;
                } else {
                    // Theirs comes first, no overlap
                    append_base_lines(
                        &mut result,
                        base_lines,
                        base_pos,
                        theirs.base_range.start,
                    );
                    append_edit(&mut result, theirs);
                    base_pos = theirs.base_range.end;
                    ti += 1;
                }
            }
            (Some(ours), None) => {
                append_base_lines(&mut result, base_lines, base_pos, ours.base_range.start);
                append_edit(&mut result, ours);
                base_pos = ours.base_range.end;
                oi += 1;
            }
            (None, Some(theirs)) => {
                append_base_lines(
                    &mut result,
                    base_lines,
                    base_pos,
                    theirs.base_range.start,
                );
                append_edit(&mut result, theirs);
                base_pos = theirs.base_range.end;
                ti += 1;
            }
            (None, None) => unreachable!(),
        }
    }

    // Append any remaining base lines after the last edit
    append_base_lines(&mut result, base_lines, base_pos, base_lines.len());

    if has_conflict {
        MergeResult::Conflict(result)
    } else {
        MergeResult::Clean(result)
    }
}

/// Returns `true` if two edit regions overlap on the base.
///
/// Two regions overlap if their base ranges intersect, or if both are
/// zero-length insertions at the same position.
fn regions_overlap(a: &EditRegion, b: &EditRegion) -> bool {
    // Two zero-length insertions at the same point conflict
    if a.base_range.is_empty() && b.base_range.is_empty() {
        return a.base_range.start == b.base_range.start;
    }
    // Standard range overlap: !(a.end <= b.start || b.end <= a.start)
    a.base_range.start < b.base_range.end && b.base_range.start < a.base_range.end
}

/// Appends base lines from `from` (inclusive) to `to` (exclusive).
fn append_base_lines(result: &mut String, base_lines: &[&str], from: usize, to: usize) {
    for line in &base_lines[from..to] {
        result.push_str(line);
    }
}

/// Appends the replacement lines from an edit region.
fn append_edit(result: &mut String, edit: &EditRegion) {
    for line in &edit.new_lines {
        result.push_str(line);
    }
}

/// Emits conflict markers with the content from both sides.
fn emit_conflict(result: &mut String, ours: &EditRegion, theirs: &EditRegion) {
    result.push_str("<<<<<<< workspace\n");
    for line in &ours.new_lines {
        result.push_str(line);
    }
    result.push_str("=======\n");
    for line in &theirs.new_lines {
        result.push_str(line);
    }
    result.push_str(">>>>>>> bundle\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_changes() {
        let base = "line1\nline2\nline3\n";
        assert_eq!(merge3(base, base, base), MergeResult::Clean(base.to_string()));
    }

    #[test]
    fn only_ours_changed() {
        let base = "line1\nline2\nline3\n";
        let ours = "line1\nmodified\nline3\n";
        let result = merge3(base, ours, base);
        assert_eq!(result, MergeResult::Clean(ours.to_string()));
    }

    #[test]
    fn only_theirs_changed() {
        let base = "line1\nline2\nline3\n";
        let theirs = "line1\nline2\nupdated\n";
        let result = merge3(base, base, theirs);
        assert_eq!(result, MergeResult::Clean(theirs.to_string()));
    }

    #[test]
    fn both_changed_different_regions() {
        let base = "line1\nline2\nline3\nline4\n";
        let ours = "MODIFIED1\nline2\nline3\nline4\n";
        let theirs = "line1\nline2\nline3\nMODIFIED4\n";
        let result = merge3(base, ours, theirs);
        assert!(result.is_clean(), "expected clean merge, got: {:?}", result);
        assert_eq!(result.content(), "MODIFIED1\nline2\nline3\nMODIFIED4\n");
    }

    #[test]
    fn both_changed_same_region_conflict() {
        let base = "line1\nline2\nline3\n";
        let ours = "line1\nours_change\nline3\n";
        let theirs = "line1\ntheirs_change\nline3\n";
        let result = merge3(base, ours, theirs);
        assert!(!result.is_clean(), "expected conflict, got: {:?}", result);
        let content = result.content();
        assert!(content.contains("<<<<<<< workspace"), "missing ours marker");
        assert!(content.contains("======="), "missing separator");
        assert!(content.contains(">>>>>>> bundle"), "missing theirs marker");
        assert!(content.contains("ours_change"), "missing ours content");
        assert!(content.contains("theirs_change"), "missing theirs content");
    }

    #[test]
    fn both_made_identical_change() {
        let base = "line1\nline2\nline3\n";
        let changed = "line1\nsame_change\nline3\n";
        let result = merge3(base, changed, changed);
        assert_eq!(result, MergeResult::Clean(changed.to_string()));
    }

    #[test]
    fn ours_added_lines() {
        let base = "line1\nline2\n";
        let ours = "line1\nline2\nline3\nline4\n";
        let result = merge3(base, ours, base);
        assert_eq!(result, MergeResult::Clean(ours.to_string()));
    }

    #[test]
    fn theirs_added_lines() {
        let base = "line1\nline2\n";
        let theirs = "line1\nline2\nextra1\nextra2\n";
        let result = merge3(base, base, theirs);
        assert_eq!(result, MergeResult::Clean(theirs.to_string()));
    }

    #[test]
    fn ours_deleted_lines() {
        let base = "line1\nline2\nline3\nline4\n";
        let ours = "line1\nline4\n";
        let result = merge3(base, ours, base);
        assert_eq!(result, MergeResult::Clean(ours.to_string()));
    }

    #[test]
    fn empty_base() {
        let base = "";
        let ours = "our content\n";
        let theirs = "their content\n";
        let result = merge3(base, ours, theirs);
        assert!(
            !result.is_clean(),
            "expected conflict when both add to empty, got: {:?}",
            result
        );
        let content = result.content();
        assert!(content.contains("<<<<<<< workspace"));
        assert!(content.contains("our content"));
        assert!(content.contains("their content"));
        assert!(content.contains(">>>>>>> bundle"));
    }
}
