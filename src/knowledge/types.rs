use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Archetype {
    Person,
    Concept,
    Decision,
    Event,
    Place,
    Project,
    Organization,
    Procedure,
    Media,
    Quote,
    Topic,
}

impl std::fmt::Display for Archetype {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Person => "person",
            Self::Concept => "concept",
            Self::Decision => "decision",
            Self::Event => "event",
            Self::Place => "place",
            Self::Project => "project",
            Self::Organization => "organization",
            Self::Procedure => "procedure",
            Self::Media => "media",
            Self::Quote => "quote",
            Self::Topic => "topic",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoteFrontMatter {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archetype: Option<Archetype>,
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
