mod error;
mod files;
mod notes;
mod parser;
pub mod reconcile;
mod types;

pub use error::KnowledgeError;
pub use files::{
    diary_path, list_diary_entries, list_references, load_diary_today, read_diary, reference_path,
    write_diary,
};
pub use notes::{
    ensure_index_notes, list_notes, note_path, note_relative_path, read_note, subfolder_from_tags,
    write_note,
};
pub use parser::{extract_wiki_links, parse_note, serialize_note, slug_from_title};
pub use types::{KnowledgeKind, NoteFrontMatter, ParsedNote, WikiLink};
