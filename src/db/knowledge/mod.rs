mod crud;
mod graph;
mod records;
mod search;
mod stats;

pub use crud::{
    append_diary, create_diary, create_note, create_note_full, create_reference, delete_note,
    delete_reference, find_note_by_title, find_reference_by_path, find_reference_by_url,
    get_diary_by_date, get_note, get_reference, list_all_diary, list_all_notes,
    list_all_references, list_diary_page, list_notes_page, list_recent, list_references_by_topic,
    list_references_page, update_note, update_reference_path,
};
pub use graph::{
    create_cited_edge, create_edge, delete_outgoing_edges, incoming_cited, incoming_edges,
    orphan_notes, outgoing_edges, related_note_ids,
};
pub use records::{DiaryRecord, EdgeRecord, NoteRecord, RecentItem, ReferenceRecord, SearchHit};
pub use search::{hybrid_merge, search_diary, search_notes, search_references};
pub use stats::{
    count_diary, count_edges, count_notes, count_references, count_stubs, list_tags_with_counts,
};
