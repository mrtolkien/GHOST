use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_SKILLS: &[(&str, &str)] = &[
    (
        "cron-job-author",
        include_str!("../prompts/skills/cron-job-author.md"),
    ),
    (
        "note-writer",
        include_str!("../prompts/skills/note-writer.md"),
    ),
    ("research", include_str!("../prompts/skills/research.md")),
    (
        "skill-creator",
        include_str!("../prompts/skills/skill-creator.md"),
    ),
];

#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// Parse YAML frontmatter from a skill file. Extracts `name` and
/// `description` per the agentskills.io spec. Returns `None` for
/// malformed frontmatter. Unknown fields (triggers, metadata, etc.)
/// are silently ignored.
pub fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    let after_open = &content[3..];
    let close = after_open.find("\n---")?;
    let block = &after_open[..close];

    let mut name = None;
    let mut description_parts: Vec<String> = Vec::new();
    let mut in_description = false;

    for line in block.lines() {
        let trimmed = line.trim();

        // Check for a new top-level key
        if !trimmed.is_empty()
            && !trimmed.starts_with('-')
            && !line.starts_with(' ')
            && !line.starts_with('\t')
        {
            in_description = false;
        }

        if let Some(value) = trimmed.strip_prefix("name:") {
            name = Some(value.trim().to_string());
            in_description = false;
        } else if trimmed.starts_with("description:") {
            in_description = true;
            let value = trimmed.strip_prefix("description:").unwrap_or("").trim();
            if !value.is_empty() {
                description_parts.push(value.to_string());
            }
        } else if in_description && (line.starts_with(' ') || line.starts_with('\t')) {
            let part = trimmed.to_string();
            if !part.is_empty() {
                description_parts.push(part);
            }
        }
    }

    let name = name.filter(|n| !n.is_empty())?;
    let description = description_parts.join(" ");

    if description.is_empty() {
        return None;
    }

    Some((name, description))
}

/// Scan `$WORKSPACE/skills/` for subdirectories containing `skill.md`,
/// parse their frontmatter, and return a sorted list of discovered skills.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn discover_skills(workspace: &Path) -> Vec<Skill> {
    let skills_dir = workspace.join("skills");

    let entries = match fs::read_dir(&skills_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let dir_name = entry.file_name().to_string_lossy().to_string();

        if dir_name.starts_with('.') {
            continue;
        }

        let skill_path = entry.path().join("skill.md");
        let content = match fs::read_to_string(&skill_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        match parse_frontmatter(&content) {
            Some((name, description)) => {
                skills.push(Skill {
                    name,
                    description,
                    path: skill_path,
                });
            }
            None => {
                logfire::warn!(
                    "Malformed skill frontmatter in {path}",
                    path = skill_path.display().to_string(),
                );
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Install default skills into `$WORKSPACE/skills/` if they don't already
/// exist. Each skill is a subdirectory with a `skill.md` file.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn install_default_skills(workspace: &Path) -> Result<(), std::io::Error> {
    let skills_dir = workspace.join("skills");

    for (name, content) in DEFAULT_SKILLS {
        let skill_dir = skills_dir.join(name);
        fs::create_dir_all(&skill_dir)?;

        let skill_file = skill_dir.join("skill.md");
        if !skill_file.exists() {
            fs::write(&skill_file, content)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_frontmatter_extracts_name_and_description() {
        let content = "\
---
name: test-skill
description: A test skill for testing.
---

# Body
";
        let (name, desc) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "test-skill");
        assert_eq!(desc, "A test skill for testing.");
    }

    #[test]
    fn parse_frontmatter_handles_multiline_description() {
        let content = "\
---
name: multi-line
description:
  This is a multiline description that
  spans several lines.
---

# Body
";
        let (name, desc) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "multi-line");
        assert_eq!(
            desc,
            "This is a multiline description that spans several lines."
        );
    }

    #[test]
    fn parse_frontmatter_ignores_unknown_fields() {
        let content = "\
---
name: with-extras
description: Has extra fields.
triggers:
  - hello world
  - do the thing
metadata:
  author: test
---

# Body
";
        let (name, desc) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "with-extras");
        assert_eq!(desc, "Has extra fields.");
    }

    #[test]
    fn parse_frontmatter_returns_none_for_malformed() {
        // No opening ---
        assert!(parse_frontmatter("name: broken").is_none());

        // Missing name
        let no_name = "\
---
description: No name here.
---
";
        assert!(parse_frontmatter(no_name).is_none());

        // Missing description
        let no_desc = "\
---
name: no-desc
---
";
        assert!(parse_frontmatter(no_desc).is_none());
    }

    #[test]
    fn discover_skills_finds_and_sorts() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");

        let zeta = skills.join("zeta-skill");
        fs::create_dir_all(&zeta).unwrap();
        fs::write(
            zeta.join("skill.md"),
            "---\nname: zeta-skill\ndescription: Zeta.\n---\n",
        )
        .unwrap();

        let alpha = skills.join("alpha-skill");
        fs::create_dir_all(&alpha).unwrap();
        fs::write(
            alpha.join("skill.md"),
            "---\nname: alpha-skill\ndescription: Alpha.\n---\n",
        )
        .unwrap();

        let found = discover_skills(dir.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "alpha-skill");
        assert_eq!(found[0].description, "Alpha.");
        assert_eq!(found[1].name, "zeta-skill");
        assert_eq!(found[1].description, "Zeta.");
    }

    #[test]
    fn discover_skills_skips_hidden_and_invalid() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");

        // Hidden directory
        let hidden = skills.join(".hidden");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(
            hidden.join("skill.md"),
            "---\nname: hidden\ndescription: Hidden.\n---\n",
        )
        .unwrap();

        // Directory without skill.md
        let empty = skills.join("empty-dir");
        fs::create_dir_all(&empty).unwrap();

        // Valid skill
        let valid = skills.join("valid");
        fs::create_dir_all(&valid).unwrap();
        fs::write(
            valid.join("skill.md"),
            "---\nname: valid\ndescription: Valid.\n---\n",
        )
        .unwrap();

        let found = discover_skills(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "valid");
    }

    #[test]
    fn install_default_skills_creates_files() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        fs::create_dir_all(&skills).unwrap();

        install_default_skills(dir.path()).unwrap();

        for (name, _) in DEFAULT_SKILLS {
            let skill_file = skills.join(name).join("skill.md");
            assert!(skill_file.exists(), "Expected {skill_file:?} to exist");

            let content = fs::read_to_string(&skill_file).unwrap();
            assert!(content.contains("---"));
        }

        assert_eq!(DEFAULT_SKILLS.len(), 4);
    }

    #[test]
    fn install_default_skills_does_not_overwrite() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        fs::create_dir_all(&skills).unwrap();

        install_default_skills(dir.path()).unwrap();

        let first_skill = DEFAULT_SKILLS[0].0;
        let skill_file = skills.join(first_skill).join("skill.md");
        fs::write(&skill_file, "custom content").unwrap();

        install_default_skills(dir.path()).unwrap();

        let content = fs::read_to_string(&skill_file).unwrap();
        assert_eq!(content, "custom content");
    }
}
