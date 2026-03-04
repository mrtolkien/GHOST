use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Paused,
    Completed,
}

impl std::fmt::Display for ProjectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

impl std::str::FromStr for ProjectStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            other => Err(format!(
                "invalid project status: {other} (expected: active, paused, completed)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Done,
    Blocked,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Todo => write!(f, "todo"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Done => write!(f, "done"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "todo" => Ok(Self::Todo),
            "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            "blocked" => Ok(Self::Blocked),
            other => Err(format!(
                "invalid task status: {other} (expected: todo, in_progress, done, blocked)"
            )),
        }
    }
}

fn default_project_status() -> ProjectStatus {
    ProjectStatus::Active
}

fn default_task_status() -> TaskStatus {
    TaskStatus::Todo
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ProjectFrontMatter {
    pub title: String,
    #[serde(default = "default_project_status")]
    pub status: ProjectStatus,
    pub created: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedProject {
    pub front: ProjectFrontMatter,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TaskFrontMatter {
    pub title: String,
    #[serde(default = "default_task_status")]
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    pub created: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTask {
    pub front: TaskFrontMatter,
    pub body: String,
}
