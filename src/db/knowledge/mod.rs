mod crud;
mod graph;
mod import_batch;
mod records;
mod search;
mod stats;
mod topics;

pub use crud::{
    append_diary, create_diary, create_note, create_note_full, create_reference, delete_diary,
    delete_note, delete_reference, find_note_by_path, find_note_by_title, find_reference_by_path,
    find_reference_by_url, get_diary_by_date, get_note, get_reference, list_all_diary,
    list_all_notes, list_all_references, list_diary_page, list_notes_page, list_recent,
    list_references_by_topic, list_references_page, update_note, update_reference_path,
};
pub use graph::{
    backfill_message_source_references, create_cited_edge, create_edge, create_message_source,
    delete_outgoing_edges, incoming_cited, incoming_edges, orphan_notes, outgoing_edges,
    related_note_ids,
};
pub use import_batch::{
    delete_import_batch, get_import_batch_by_topic, list_import_batches, upsert_import_batch,
};
pub use records::{
    DiaryRecord, EdgeRecord, ImportBatchRecord, NoteRecord, RecentItem, ReferenceRecord, ScriptRecord,
    SearchHit, TopicRecord,
};
pub use search::{hybrid_merge, search_diary, search_notes, search_references, search_topics};
pub use stats::{
    count_diary, count_edges, count_notes, count_references, count_stubs, list_tags_with_counts,
};
pub use topics::{
    TopicInfo, count_references_by_topic, create_topic, delete_references_by_topic, delete_topic,
    find_or_create_topic, find_topic_by_name, find_topics_by_prefix, get_topic, list_topics,
};
