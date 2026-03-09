use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub available: Option<String>,
    pub path: PathBuf,
}

/// Parse YAML frontmatter from a skill file. Extracts `name` and
/// `description` per the agentskills.io spec. Returns `None` for
/// malformed frontmatter. Unknown fields (triggers, metadata, etc.)
/// are silently ignored.
pub fn parse_frontmatter(content: &str) -> Option<(String, String, Option<String>)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    let after_open = &content[3..];
    let close = after_open.find("\n---")?;
    let block = &after_open[..close];

    let mut name = None;
    let mut available = None;
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
        } else if let Some(value) = trimmed.strip_prefix("available:") {
            let value = value.trim();
            if !value.is_empty() {
                available = Some(value.to_string());
            }
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

    Some((name, description, available))
}

/// Scan `$WORKSPACE/skills/` for subdirectories containing `skill.md`,
/// parse their frontmatter, and return a sorted list of discovered skills.
/// Recurses into namespace directories (those without `skill.md`).
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn discover_skills(workspace: &Path) -> Vec<Skill> {
    let skills_dir = workspace.join("skills");
    let mut skills = Vec::new();
    walk_skills_dir(&skills_dir, &mut skills);
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Recursively walk a skills directory. If a subdirectory contains
/// `skill.md` it is a leaf skill; otherwise it is a namespace and we
/// recurse into it. Directories starting with `.` are skipped.
pub(crate) fn walk_skills_dir(dir: &Path, skills: &mut Vec<Skill>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let dir_name = entry.file_name().to_string_lossy().to_string();

        if dir_name.starts_with('.') {
            continue;
        }

        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let skill_path = entry_path.join("skill.md");
        if skill_path.exists() {
            // Leaf skill
            let content = match fs::read_to_string(&skill_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            match parse_frontmatter(&content) {
                Some((name, description, available)) => {
                    skills.push(Skill {
                        name,
                        description,
                        available,
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
        } else {
            // Namespace directory — recurse
            walk_skills_dir(&entry_path, skills);
        }
    }
}

/// Collect extra files in a skill directory for the `<extra-files>` block.
///
/// Walks `skill_dir` recursively, returning `./`-relative paths for all
/// files except `skill.md` and anything inside agent directories (dirs
/// containing `agent.lua`). Returns sorted paths; empty vec if no extras.
pub fn collect_extras(skill_dir: &Path) -> Vec<PathBuf> {
    let mut extras = Vec::new();
    walk_extras(skill_dir, skill_dir, &mut extras);
    extras.sort();
    extras
}

fn walk_extras(base: &Path, dir: &Path, extras: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    // Skip this directory entirely if it contains agent.lua
    if dir != base && dir.join("agent.lua").exists() {
        return;
    }

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            walk_extras(base, &path, extras);
        } else {
            let name = entry.file_name();
            let name = name.to_string_lossy();

            // Skip skill.md itself
            if name == "skill.md" {
                continue;
            }

            if let Ok(rel) = path.strip_prefix(base) {
                extras.push(PathBuf::from(".").join(rel));
            }
        }
    }
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
        let (name, desc, available) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "test-skill");
        assert_eq!(desc, "A test skill for testing.");
        assert!(available.is_none());
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
        let (name, desc, _) = parse_frontmatter(content).unwrap();
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
        let (name, desc, _) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "with-extras");
        assert_eq!(desc, "Has extra fields.");
    }

    #[test]
    fn parse_frontmatter_extracts_available() {
        let content = "\
---
name: coding-only
description: Only for coding agent.
available: coding
---

# Body
";
        let (name, desc, available) = parse_frontmatter(content).unwrap();
        assert_eq!(name, "coding-only");
        assert_eq!(desc, "Only for coding agent.");
        assert_eq!(available.as_deref(), Some("coding"));
    }

    #[test]
    fn parse_frontmatter_defaults_available_to_none() {
        let content = "\
---
name: general
description: Available everywhere.
---

# Body
";
        let (_, _, available) = parse_frontmatter(content).unwrap();
        assert!(available.is_none());
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
    fn discover_skills_finds_nested() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");

        // Top-level skill
        let top = skills.join("top-skill");
        fs::create_dir_all(&top).unwrap();
        fs::write(
            top.join("skill.md"),
            "---\nname: top-skill\ndescription: Top.\n---\n",
        )
        .unwrap();

        // Nested skill under a namespace
        let nested = skills.join("superpowers").join("nested-skill");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("skill.md"),
            "---\nname: nested-skill\ndescription: Nested.\n---\n",
        )
        .unwrap();

        let found = discover_skills(dir.path());
        assert_eq!(found.len(), 2);
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"top-skill"));
        assert!(names.contains(&"nested-skill"));
    }

    #[test]
    fn bundled_install_creates_skill_files() {
        let dir = TempDir::new().unwrap();
        crate::bundled::install_all(dir.path()).unwrap();

        let skills = dir.path().join("skills");

        // Spot-check key skill files exist
        assert!(skills.join("agent-creator/skill.md").exists());
        assert!(skills.join("coding/skill.md").exists());
        assert!(skills.join("deep-research/skill.md").exists());
        assert!(skills.join("nix-shell/skill.md").exists());

        // Verify skill files have frontmatter
        let content = fs::read_to_string(skills.join("coding/skill.md")).unwrap();
        assert!(content.contains("---"), "skill.md should have frontmatter");
    }

    #[test]
    fn bundled_install_creates_agent_subdirs() {
        let dir = TempDir::new().unwrap();
        crate::bundled::install_all(dir.path()).unwrap();

        let skills = dir.path().join("skills");

        // Deep-research agents
        assert!(
            skills
                .join("deep-research/deep-research/agent.lua")
                .exists()
        );
        assert!(
            skills
                .join("deep-research/deep-research-reflection/agent.lua")
                .exists()
        );

        // Coding agents
        assert!(
            skills
                .join("superpowers/subagent-development/coding-implementer/agent.lua")
                .exists()
        );
        assert!(
            skills
                .join("superpowers/subagent-development/coding-reviewer/agent.lua")
                .exists()
        );
    }

    #[test]
    fn collect_extras_finds_non_agent_files() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("skills").join("my-skill");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::create_dir_all(skill_dir.join("my-agent")).unwrap();

        // Skill file (excluded from extras)
        fs::write(
            skill_dir.join("skill.md"),
            "---\nname: my-skill\ndescription: Test.\n---\n",
        )
        .unwrap();
        // Extra files (included)
        fs::write(skill_dir.join("reference.md"), "ref").unwrap();
        fs::write(skill_dir.join("schema.sql"), "CREATE TABLE").unwrap();
        fs::write(skill_dir.join("scripts/run.py"), "print()").unwrap();
        // Agent dir (excluded entirely)
        fs::write(skill_dir.join("my-agent/agent.lua"), "return {}").unwrap();
        fs::write(skill_dir.join("my-agent/prompt.md"), "prompt").unwrap();

        let extras = collect_extras(&skill_dir);
        let paths: Vec<String> = extras.iter().map(|p| p.display().to_string()).collect();

        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&"./reference.md".to_string()));
        assert!(paths.contains(&"./schema.sql".to_string()));
        assert!(paths.contains(&"./scripts/run.py".to_string()));
    }

    #[test]
    fn collect_extras_empty_when_no_extras() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("skills").join("simple");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.md"),
            "---\nname: simple\ndescription: Test.\n---\n",
        )
        .unwrap();

        let extras = collect_extras(&skill_dir);
        assert!(extras.is_empty());
    }

    #[test]
    fn collect_extras_skips_nested_agent_dirs() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("skills").join("complex");
        fs::create_dir_all(skill_dir.join("agent-a")).unwrap();
        fs::create_dir_all(skill_dir.join("agent-b")).unwrap();

        fs::write(
            skill_dir.join("skill.md"),
            "---\nname: complex\ndescription: Test.\n---\n",
        )
        .unwrap();
        fs::write(skill_dir.join("agent-a/agent.lua"), "return {}").unwrap();
        fs::write(skill_dir.join("agent-a/prompt.md"), "p").unwrap();
        fs::write(skill_dir.join("agent-b/agent.lua"), "return {}").unwrap();
        fs::write(skill_dir.join("agent-b/user-message.md"), "u").unwrap();

        let extras = collect_extras(&skill_dir);
        assert!(extras.is_empty());
    }

    #[test]
    fn bundled_install_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        crate::bundled::install_all(dir.path()).unwrap();

        let first = &crate::bundled::bundled_files()[0];
        let file = dir.path().join(first.path);
        fs::write(&file, "custom content").unwrap();

        crate::bundled::install_all(dir.path()).unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert_ne!(content, "custom content", "should overwrite existing files");
    }
}
