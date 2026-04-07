mod crud;
mod graph;
mod import_batch;
mod records;
mod search;
mod stats;
mod topics;

pub use crud::{
    CodeFileHashRecord, FileHashRecord, NoteInput, append_diary, create_code_file, create_diary,
    create_note, create_note_full, create_reference, create_script, delete_code_file,
    delete_code_files_by_repo, delete_diary, delete_note, delete_reference, delete_script,
    find_code_file, find_note_by_path, find_note_by_title, find_reference_by_path,
    find_reference_by_url, find_script_by_path, get_diary_by_date, get_note, get_reference,
    get_script, list_all_diary, list_all_notes, list_all_references, list_all_scripts, list_recent,
    list_references_by_topic, load_code_file_hashes, load_diary_file_hashes, load_note_file_hashes,
    load_reference_file_hashes, load_script_file_hashes, update_code_file, update_diary,
    update_note, update_reference, update_reference_import_batch, update_reference_path,
    update_references_import_batch_by_topic, update_script,
};
pub use graph::{
    backfill_message_source_references, cited_reference_ids, create_cited_edge, create_edge,
    create_message_source, delete_outgoing_edges, incoming_cited, incoming_edges, orphan_notes,
    outgoing_edges, related_note_ids,
};
pub use import_batch::{
    delete_import_batch, get_import_batch_by_topic, list_import_batches, upsert_import_batch,
};
pub use records::{
    CodeFileRecord, DiaryRecord, EdgeRecord, ImportBatchRecord, NoteRecord, RecentItem,
    ReferenceRecord, ScriptRecord, SearchHit, TopicRecord,
};
pub use search::{
    hybrid_merge, search_code_files, search_diary, search_notes, search_references, search_scripts,
    search_topics,
};
pub use stats::{
    count_diary, count_edges, count_notes, count_references, count_scripts, count_stubs,
    list_tags_with_counts,
};
pub use topics::{
    TopicInfo, count_references_by_topic, create_topic, delete_references_by_topic, delete_topic,
    find_or_create_topic, find_topic_by_name, find_topics_by_prefix, get_topic, list_topics,
};
