use super::error::ProjectError;
use super::types::{ParsedProject, ParsedTask, ProjectFrontMatter, TaskFrontMatter};

const DELIMITER: &str = "---";

pub fn parse_project(raw: &str) -> Result<ParsedProject, ProjectError> {
    let (front, body) = parse_frontmatter::<ProjectFrontMatter>(raw)?;
    Ok(ParsedProject { front, body })
}

pub fn serialize_project(front: &ProjectFrontMatter, body: &str) -> Result<String, ProjectError> {
    serialize_frontmatter(front, body)
}

pub fn parse_task(raw: &str) -> Result<ParsedTask, ProjectError> {
    let (front, body) = parse_frontmatter::<TaskFrontMatter>(raw)?;
    Ok(ParsedTask { front, body })
}

pub fn serialize_task(front: &TaskFrontMatter, body: &str) -> Result<String, ProjectError> {
    serialize_frontmatter(front, body)
}

/// Extract task slugs from a PRIORITY.md file. Parses numbered list items
/// (e.g. `1. some_task`), ignoring headers and description lines.
#[must_use]
pub fn parse_priority_list(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (num_part, rest) = trimmed.split_once(". ")?;
            if !num_part.is_empty() && num_part.chars().all(|c| c.is_ascii_digit()) {
                let slug = rest.trim();
                if !slug.is_empty() {
                    return Some(slug.to_string());
                }
            }
            None
        })
        .collect()
}

#[must_use]
pub fn serialize_priority_list(slugs: &[String]) -> String {
    let mut output = String::from(
        "# Task Priority\n\n\
         Tasks in priority order (highest first). Unlisted tasks are unprioritized.\n\
         Reorder freely by editing this file.\n\n",
    );
    for (i, slug) in slugs.iter().enumerate() {
        output.push_str(&format!("{}. {slug}\n", i + 1));
    }
    output
}

fn parse_frontmatter<T: serde::de::DeserializeOwned>(
    raw: &str,
) -> Result<(T, String), ProjectError> {
    let trimmed = raw.trim_start();

    if !trimmed.starts_with(DELIMITER) {
        return Err(ProjectError::InvalidFrontMatter {
            reason: "missing opening --- delimiter".to_string(),
        });
    }

    let after_open = &trimmed[DELIMITER.len()..];
    let Some(end_pos) = after_open.find(DELIMITER) else {
        return Err(ProjectError::InvalidFrontMatter {
            reason: "missing closing --- delimiter".to_string(),
        });
    };

    let frontmatter_str = &after_open[..end_pos];
    let body_start = DELIMITER.len() + end_pos + DELIMITER.len();
    let body = trimmed[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&trimmed[body_start..])
        .to_string();

    let front: T = serde_yaml::from_str(frontmatter_str)
        .map_err(|source| ProjectError::FrontMatterParse { source })?;

    Ok((front, body))
}

fn serialize_frontmatter<T: serde::Serialize>(
    front: &T,
    body: &str,
) -> Result<String, ProjectError> {
    let yaml_str = serde_yaml::to_string(front)
        .map_err(|source| ProjectError::FrontMatterSerialize { source })?;
    Ok(format!("{DELIMITER}\n{yaml_str}{DELIMITER}\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::types::{ProjectStatus, TaskStatus};

    #[test]
    fn parse_project_roundtrip() {
        let raw = "---\ntitle: Build Website\nstatus: active\ncreated: '2026-03-04'\ntags:\n- web\n---\nGoals and scope.\n";
        let parsed = parse_project(raw).unwrap();
        assert_eq!(parsed.front.title, "Build Website");
        assert_eq!(parsed.front.status, ProjectStatus::Active);
        assert_eq!(parsed.front.tags, vec!["web"]);

        let serialized = serialize_project(&parsed.front, &parsed.body).unwrap();
        let reparsed = parse_project(&serialized).unwrap();
        assert_eq!(reparsed.front, parsed.front);
    }

    #[test]
    fn parse_task_roundtrip() {
        let raw = "---\ntitle: Set up DNS\nstatus: todo\ncreated: '2026-03-04'\n---\nDNS spec.\n";
        let parsed = parse_task(raw).unwrap();
        assert_eq!(parsed.front.title, "Set up DNS");
        assert_eq!(parsed.front.status, TaskStatus::Todo);
        assert!(parsed.front.blocked_by.is_empty());

        let serialized = serialize_task(&parsed.front, &parsed.body).unwrap();
        let reparsed = parse_task(&serialized).unwrap();
        assert_eq!(reparsed.front, parsed.front);
    }

    #[test]
    fn parse_task_with_blocked_by() {
        let raw = "---\ntitle: Deploy\nstatus: blocked\nblocked_by:\n- setup_dns\ncreated: '2026-03-04'\n---\nBlocked.\n";
        let parsed = parse_task(raw).unwrap();
        assert_eq!(parsed.front.blocked_by, vec!["setup_dns"]);
    }

    #[test]
    fn priority_list_roundtrip() {
        let slugs = vec![
            "setup_dns".to_string(),
            "design_homepage".to_string(),
            "deploy".to_string(),
        ];
        let serialized = serialize_priority_list(&slugs);
        let parsed = parse_priority_list(&serialized);
        assert_eq!(parsed, slugs);
    }

    #[test]
    fn priority_list_ignores_non_numbered_lines() {
        let raw = "# Header\n\nSome description.\n\n1. first_task\n2. second_task\n\nExtra text.\n";
        let parsed = parse_priority_list(raw);
        assert_eq!(parsed, vec!["first_task", "second_task"]);
    }

    #[test]
    fn missing_delimiter_error() {
        let result = parse_project("title: Oops\n\nBody.");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing opening"));
    }

    #[test]
    fn default_status_values() {
        let raw = "---\ntitle: Minimal\ncreated: '2026-03-04'\n---\nBody.\n";
        let project = parse_project(raw).unwrap();
        assert_eq!(project.front.status, ProjectStatus::Active);

        let task = parse_task(raw).unwrap();
        assert_eq!(task.front.status, TaskStatus::Todo);
    }
}
