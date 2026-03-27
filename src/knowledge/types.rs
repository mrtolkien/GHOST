use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Archetype {
    Entity,
    Analysis,
    Source,
    Profile,
    Topic,
}

impl Archetype {
    /// Default trust score for this archetype.
    #[must_use]
    pub fn default_trust(self) -> i64 {
        match self {
            Self::Entity | Self::Source | Self::Topic => 5,
            Self::Analysis => 4,
            Self::Profile => 6,
        }
    }
}

impl fmt::Display for Archetype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Entity => "entity",
            Self::Analysis => "analysis",
            Self::Source => "source",
            Self::Profile => "profile",
            Self::Topic => "topic",
        };
        f.write_str(s)
    }
}

impl FromStr for Archetype {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "entity" => Ok(Self::Entity),
            "analysis" => Ok(Self::Analysis),
            "source" => Ok(Self::Source),
            "profile" => Ok(Self::Profile),
            "topic" => Ok(Self::Topic),
            other => Err(format!("unknown archetype '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoteFrontMatter {
    pub title: String,
    pub archetype: Archetype,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default = "default_trust")]
    pub trust: i64,
    pub written_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
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
