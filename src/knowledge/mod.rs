mod error;
mod files;
mod parser;
pub mod reconcile;
mod types;

pub use error::KnowledgeError;
pub use files::{
    diary_path, ensure_index_notes, list_diary_entries, list_notes, list_references,
    load_diary_today, note_path, note_relative_path, read_diary, read_note, reference_path,
    subfolder_from_tags, write_diary, write_note,
};
pub use parser::{extract_wiki_links, parse_note, serialize_note, slug_from_title};
pub use types::{Archetype, KnowledgeKind, NoteFrontMatter, ParsedNote, WikiLink};
