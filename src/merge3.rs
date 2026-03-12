/// Line-based three-way merge.
///
/// Compares `base` (common ancestor) against `ours` (local edits) and
/// `theirs` (incoming changes). Produces either a clean merge or conflict
/// markers in the style of `git merge`.
use similar::TextDiff;

/// Result of a three-way merge.
pub enum MergeResult {
    /// Both sides changed without overlapping — merged cleanly.
    Clean(String),
    /// Overlapping edits — result contains conflict markers.
    Conflict(String),
}

/// Perform a line-based three-way merge.
///
/// - `base`: the common ancestor (old bundle / shadow copy)
/// - `ours`: local version (workspace file, possibly user-edited)
/// - `theirs`: incoming version (new bundle)
///
/// If only one side changed a region, that change wins. If both sides
/// changed the same region, conflict markers are inserted.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> MergeResult {
    // Fast paths
    if base == theirs {
        return MergeResult::Clean(ours.to_string());
    }
    if base == ours {
        return MergeResult::Clean(theirs.to_string());
    }
    if ours == theirs {
        return MergeResult::Clean(ours.to_string());
    }

    let diff_ours = TextDiff::from_lines(base, ours);
    let diff_theirs = TextDiff::from_lines(base, theirs);

    let our_ops = collect_ops(&diff_ours);
    let their_ops = collect_ops(&diff_theirs);

    let mut result = String::new();
    let mut has_conflict = false;

    let mut our_idx = 0;
    let mut their_idx = 0;

    while our_idx < our_ops.len() || their_idx < their_ops.len() {
        let our_op = our_ops.get(our_idx);
        let their_op = their_ops.get(their_idx);

        match (our_op, their_op) {
            (Some(op_a), Some(op_b)) => {
                if op_a.base_range() == op_b.base_range() {
                    // Same base region
                    match (&op_a, &op_b) {
                        (MergeOp::Equal(text), MergeOp::Equal(_)) => {
                            result.push_str(text);
                            our_idx += 1;
                            their_idx += 1;
                        }
                        (MergeOp::Equal(_), MergeOp::Changed { new, .. }) => {
                            // Only theirs changed — take theirs
                            result.push_str(new);
                            our_idx += 1;
                            their_idx += 1;
                        }
                        (MergeOp::Changed { new, .. }, MergeOp::Equal(_)) => {
                            // Only ours changed — take ours
                            result.push_str(new);
                            our_idx += 1;
                            their_idx += 1;
                        }
                        (
                            MergeOp::Changed { new: our_new, .. },
                            MergeOp::Changed { new: their_new, .. },
                        ) => {
                            if our_new == their_new {
                                // Both made the same change
                                result.push_str(our_new);
                            } else {
                                // Conflict
                                has_conflict = true;
                                result.push_str("<<<<<<< workspace\n");
                                result.push_str(our_new);
                                if !our_new.ends_with('\n') {
                                    result.push('\n');
                                }
                                result.push_str("=======\n");
                                result.push_str(their_new);
                                if !their_new.ends_with('\n') {
                                    result.push('\n');
                                }
                                result.push_str(">>>>>>> bundle\n");
                            }
                            our_idx += 1;
                            their_idx += 1;
                        }
                    }
                } else {
                    // Different base ranges — emit whichever starts first
                    let (a_start, _) = op_a.base_range();
                    let (b_start, _) = op_b.base_range();
                    if a_start <= b_start {
                        emit_op(&mut result, op_a);
                        our_idx += 1;
                    } else {
                        emit_op(&mut result, op_b);
                        their_idx += 1;
                    }
                }
            }
            (Some(op), None) => {
                emit_op(&mut result, op);
                our_idx += 1;
            }
            (None, Some(op)) => {
                emit_op(&mut result, op);
                their_idx += 1;
            }
            (None, None) => break,
        }
    }

    if has_conflict {
        MergeResult::Conflict(result)
    } else {
        MergeResult::Clean(result)
    }
}

/// A merge operation — either an unchanged region or a changed one.
#[derive(Debug)]
enum MergeOp {
    Equal(String),
    Changed {
        base_start: usize,
        base_end: usize,
        new: String,
    },
}

impl MergeOp {
    fn base_range(&self) -> (usize, usize) {
        match self {
            MergeOp::Equal(_) => (0, 0), // sentinel — only compared when both are Equal
            MergeOp::Changed {
                base_start,
                base_end,
                ..
            } => (*base_start, *base_end),
        }
    }
}

fn emit_op(result: &mut String, op: &MergeOp) {
    match op {
        MergeOp::Equal(text) => result.push_str(text),
        MergeOp::Changed { new, .. } => result.push_str(new),
    }
}

/// Collect diff ops from a `TextDiff`, grouping consecutive changes
/// by their base line position.
fn collect_ops(diff: &TextDiff<'_, '_, '_, str>) -> Vec<MergeOp> {
    let mut ops = Vec::new();

    for op in diff.ops() {
        match op {
            similar::DiffOp::Equal { old_index, len, .. } => {
                let mut text = String::new();
                for i in *old_index..(*old_index + *len) {
                    text.push_str(diff.old_slices()[i]);
                }
                ops.push(MergeOp::Equal(text));
            }
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => {
                ops.push(MergeOp::Changed {
                    base_start: *old_index,
                    base_end: *old_index + *old_len,
                    new: String::new(),
                });
            }
            similar::DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => {
                let mut text = String::new();
                for i in *new_index..(*new_index + *new_len) {
                    text.push_str(diff.new_slices()[i]);
                }
                ops.push(MergeOp::Changed {
                    base_start: *old_index,
                    base_end: *old_index,
                    new: text,
                });
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let mut text = String::new();
                for i in *new_index..(*new_index + *new_len) {
                    text.push_str(diff.new_slices()[i]);
                }
                ops.push(MergeOp::Changed {
                    base_start: *old_index,
                    base_end: *old_index + *old_len,
                    new: text,
                });
            }
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_changes() {
        let base = "line 1\nline 2\nline 3\n";
        match merge3(base, base, base) {
            MergeResult::Clean(result) => assert_eq!(result, base),
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn only_ours_changed() {
        let base = "line 1\nline 2\nline 3\n";
        let ours = "line 1\nmodified\nline 3\n";
        match merge3(base, ours, base) {
            MergeResult::Clean(result) => assert_eq!(result, ours),
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn only_theirs_changed() {
        let base = "line 1\nline 2\nline 3\n";
        let theirs = "line 1\nline 2\nupdated\n";
        match merge3(base, base, theirs) {
            MergeResult::Clean(result) => assert_eq!(result, theirs),
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn both_changed_different_regions() {
        let base = "line 1\nline 2\nline 3\nline 4\n";
        let ours = "modified 1\nline 2\nline 3\nline 4\n";
        let theirs = "line 1\nline 2\nline 3\nmodified 4\n";
        match merge3(base, ours, theirs) {
            MergeResult::Clean(result) => {
                assert!(result.contains("modified 1"));
                assert!(result.contains("modified 4"));
            }
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }

    #[test]
    fn both_changed_same_region_conflict() {
        let base = "line 1\nline 2\nline 3\n";
        let ours = "line 1\nours\nline 3\n";
        let theirs = "line 1\ntheirs\nline 3\n";
        match merge3(base, ours, theirs) {
            MergeResult::Clean(_) => panic!("expected conflict"),
            MergeResult::Conflict(result) => {
                assert!(result.contains("<<<<<<< workspace"));
                assert!(result.contains("ours"));
                assert!(result.contains("======="));
                assert!(result.contains("theirs"));
                assert!(result.contains(">>>>>>> bundle"));
            }
        }
    }

    #[test]
    fn both_made_same_change() {
        let base = "line 1\nline 2\nline 3\n";
        let changed = "line 1\nsame change\nline 3\n";
        match merge3(base, changed, changed) {
            MergeResult::Clean(result) => assert_eq!(result, changed),
            MergeResult::Conflict(_) => panic!("expected clean merge"),
        }
    }
}
