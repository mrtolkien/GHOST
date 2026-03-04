use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoteFrontMatter {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default = "default_trust")]
    pub trust: i64,
}

fn default_trust() -> i64 {
    5
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedNote {
    pub front: NoteFrontMatter,
    pub body: String,
    pub wiki_links: Vec<WikiLink>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub relationship: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeKind {
    Note,
    Reference,
    Diary,
}

impl std::fmt::Display for KnowledgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Note => write!(f, "note"),
            Self::Reference => write!(f, "reference"),
            Self::Diary => write!(f, "diary"),
        }
    }
}
