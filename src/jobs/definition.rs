use std::path::Path;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::error::JobError;

const DELIMITER: &str = "+++";

#[derive(Debug, Clone, Deserialize)]
struct JobFrontmatter {
    name: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    schedule: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_tools")]
    tools: String,
    #[serde(default)]
    carry_last_output: bool,
}

fn default_enabled() -> bool {
    true
}

fn default_model() -> String {
    "default".to_string()
}

fn default_tools() -> String {
    "chat".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobToolSet {
    Chat,
    None,
}

#[derive(Debug, Clone)]
pub struct JobDefinition {
    pub name: String,
    pub enabled: bool,
    pub schedule: cron::Schedule,
    pub schedule_raw: String,
    pub model: String,
    pub tools: JobToolSet,
    pub carry_last_output: bool,
    pub prompt: String,
    pub file_stem: String,
}

pub fn parse_job_file(path: &Path, content: &str) -> Result<JobDefinition, JobError> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with(DELIMITER) {
        return Err(JobError::InvalidFrontMatter {
            reason: "missing opening +++ delimiter".to_string(),
        });
    }

    let after_open = &trimmed[DELIMITER.len()..];
    let Some(end_pos) = after_open.find(DELIMITER) else {
        return Err(JobError::InvalidFrontMatter {
            reason: "missing closing +++ delimiter".to_string(),
        });
    };

    let frontmatter_str = &after_open[..end_pos];
    let body_start = DELIMITER.len() + end_pos + DELIMITER.len();
    let body = trimmed[body_start..]
        .strip_prefix('\n')
        .unwrap_or(&trimmed[body_start..])
        .to_string();

    let front: JobFrontmatter =
        toml::from_str(frontmatter_str).map_err(|source| JobError::FrontMatterParse { source })?;

    let tools = match front.tools.as_str() {
        "none" => JobToolSet::None,
        _ => JobToolSet::Chat,
    };

    let schedule = parse_schedule(&front.schedule)?;

    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(JobDefinition {
        name: front.name,
        enabled: front.enabled,
        schedule,
        schedule_raw: front.schedule.clone(),
        model: front.model,
        tools,
        carry_last_output: front.carry_last_output,
        prompt: body,
        file_stem,
    })
}

/// Parse a 5-field cron expression by prepending "0 " for the seconds field
/// that the `cron` crate requires (6-field format).
fn parse_schedule(expr: &str) -> Result<cron::Schedule, JobError> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(JobError::InvalidSchedule {
            expression: expr.to_string(),
            reason: format!("expected 5 fields, got {}", fields.len()),
        });
    }

    let six_field = format!("0 {expr}");
    cron::Schedule::from_str(&six_field).map_err(|e| JobError::InvalidSchedule {
        expression: expr.to_string(),
        reason: e.to_string(),
    })
}

#[must_use]
pub fn next_run_after(schedule: &cron::Schedule, after: &DateTime<Utc>) -> Option<DateTime<Utc>> {
    schedule.after(after).next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_path() -> PathBuf {
        PathBuf::from("weekly-research.md")
    }

    #[test]
    fn parse_valid_job_all_fields() {
        let content = r#"+++
name = "Weekly Research"
enabled = false
schedule = "0 9 * * MON"
model = "fast"
tools = "none"
carry_last_output = true
+++
Research the latest AI papers and summarize findings.
"#;
        let def = parse_job_file(&test_path(), content).unwrap();
        assert_eq!(def.name, "Weekly Research");
        assert!(!def.enabled);
        assert_eq!(def.schedule_raw, "0 9 * * MON");
        assert_eq!(def.model, "fast");
        assert_eq!(def.tools, JobToolSet::None);
        assert!(def.carry_last_output);
        assert!(def.prompt.contains("Research the latest AI papers"));
        assert_eq!(def.file_stem, "weekly-research");
    }

    #[test]
    fn parse_valid_job_defaults() {
        let content = r#"+++
name = "Simple Job"
schedule = "*/30 * * * *"
+++
Do something every 30 minutes.
"#;
        let def = parse_job_file(&test_path(), content).unwrap();
        assert_eq!(def.name, "Simple Job");
        assert!(def.enabled);
        assert_eq!(def.model, "default");
        assert_eq!(def.tools, JobToolSet::Chat);
        assert!(!def.carry_last_output);
    }

    #[test]
    fn parse_missing_opening_delimiter() {
        let content = "name = \"Oops\"\nschedule = \"* * * * *\"\n";
        let err = parse_job_file(&test_path(), content).unwrap_err();
        assert!(err.to_string().contains("missing opening"));
    }

    #[test]
    fn parse_missing_closing_delimiter() {
        let content = "+++\nname = \"Oops\"\nschedule = \"* * * * *\"\n";
        let err = parse_job_file(&test_path(), content).unwrap_err();
        assert!(err.to_string().contains("missing closing"));
    }

    #[test]
    fn parse_missing_required_field() {
        let content = "+++\nschedule = \"* * * * *\"\n+++\nBody\n";
        let err = parse_job_file(&test_path(), content).unwrap_err();
        assert!(matches!(err, JobError::FrontMatterParse { .. }));
    }

    #[test]
    fn parse_invalid_cron_expression() {
        let content = "+++\nname = \"Bad\"\nschedule = \"not a cron\"\n+++\nBody\n";
        let err = parse_job_file(&test_path(), content).unwrap_err();
        assert!(matches!(err, JobError::InvalidSchedule { .. }));
    }

    #[test]
    fn parse_six_field_cron_rejected() {
        let content = "+++\nname = \"Six\"\nschedule = \"0 0 9 * * MON\"\n+++\nBody\n";
        let err = parse_job_file(&test_path(), content).unwrap_err();
        assert!(matches!(err, JobError::InvalidSchedule { .. }));
        assert!(err.to_string().contains("expected 5 fields, got 6"));
    }

    #[test]
    fn next_run_computation() {
        let content = "+++\nname = \"Test\"\nschedule = \"0 9 * * *\"\n+++\nBody\n";
        let def = parse_job_file(&test_path(), content).unwrap();
        let now = Utc::now();
        let next = next_run_after(&def.schedule, &now);
        assert!(next.is_some());
        let next = next.unwrap();
        assert!(next > now);
        assert_eq!(next.format("%H:%M").to_string(), "09:00");
    }
}
