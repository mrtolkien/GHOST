use std::fs;
use std::path::{Path, PathBuf};

struct DefaultSkill {
    path: &'static str,
    files: &'static [(&'static str, &'static str)],
}

const DEFAULT_SKILLS: &[DefaultSkill] = &[
    DefaultSkill {
        path: "agent-creator",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/agent-creator.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/brainstorming",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/brainstorming/skill.md"),
        )],
    },
    DefaultSkill {
        path: "coding",
        files: &[("skill.md", include_str!("../prompts/skills/coding.md"))],
    },
    DefaultSkill {
        path: "deep-research",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/deep-research.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/executing-plans",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/executing-plans/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/finishing-branch",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/finishing-branch/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/git-worktrees",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/git-worktrees/skill.md"),
        )],
    },
    DefaultSkill {
        path: "knowledge-navigator",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/knowledge-navigator.md"),
        )],
    },
    DefaultSkill {
        path: "nix-shell",
        files: &[("skill.md", include_str!("../prompts/skills/nix-shell.md"))],
    },
    DefaultSkill {
        path: "note-writer",
        files: &[("skill.md", include_str!("../prompts/skills/note-writer.md"))],
    },
    DefaultSkill {
        path: "superpowers/parallel-agents",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/parallel-agents/skill.md"),
        )],
    },
    DefaultSkill {
        path: "project-manager",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/project-manager.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/receiving-review",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/receiving-review/skill.md"),
        )],
    },
    DefaultSkill {
        path: "reference-import",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/reference-import.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/requesting-review",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/requesting-review/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/subagent-development",
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/superpowers/subagent-development/skill.md"),
            ),
            (
                "implementer-prompt.md",
                include_str!("../prompts/skills/superpowers/subagent-development/implementer-prompt.md"),
            ),
            (
                "spec-reviewer-prompt.md",
                include_str!("../prompts/skills/superpowers/subagent-development/spec-reviewer-prompt.md"),
            ),
            (
                "code-quality-reviewer-prompt.md",
                include_str!("../prompts/skills/superpowers/subagent-development/code-quality-reviewer-prompt.md"),
            ),
        ],
    },
    DefaultSkill {
        path: "superpowers/systematic-debugging",
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/superpowers/systematic-debugging/skill.md"),
            ),
            (
                "root-cause-tracing.md",
                include_str!("../prompts/skills/superpowers/systematic-debugging/root-cause-tracing.md"),
            ),
            (
                "condition-based-waiting.md",
                include_str!("../prompts/skills/superpowers/systematic-debugging/condition-based-waiting.md"),
            ),
            (
                "defense-in-depth.md",
                include_str!("../prompts/skills/superpowers/systematic-debugging/defense-in-depth.md"),
            ),
        ],
    },
    DefaultSkill {
        path: "superpowers/tdd",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/tdd/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/verification",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/verification/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/writing-plans",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/writing-plans/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/writing-skills",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/superpowers/writing-skills/skill.md"),
        )],
    },
];

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

/// Install default skills into `$WORKSPACE/skills/`, always overwriting
/// with the binary's built-in versions.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn install_default_skills(workspace: &Path) -> Result<(), std::io::Error> {
    let skills_dir = workspace.join("skills");

    for skill in DEFAULT_SKILLS {
        let skill_dir = skills_dir.join(skill.path);
        fs::create_dir_all(&skill_dir)?;
        for (filename, content) in skill.files {
            fs::write(skill_dir.join(filename), content)?;
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
    fn install_default_skills_creates_files() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        fs::create_dir_all(&skills).unwrap();

        install_default_skills(dir.path()).unwrap();

        for skill in DEFAULT_SKILLS {
            for (filename, _) in skill.files {
                let file = skills.join(skill.path).join(filename);
                assert!(file.exists(), "Expected {file:?} to exist");

                let content = fs::read_to_string(&file).unwrap();
                assert!(content.contains("---") || !filename.ends_with(".md"));
            }
        }

        assert_eq!(DEFAULT_SKILLS.len(), 21);
    }

    #[test]
    fn install_default_skills_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        fs::create_dir_all(&skills).unwrap();

        install_default_skills(dir.path()).unwrap();

        let first_skill = DEFAULT_SKILLS[0].path;
        let skill_file = skills.join(first_skill).join("skill.md");
        fs::write(&skill_file, "custom content").unwrap();

        install_default_skills(dir.path()).unwrap();

        let content = fs::read_to_string(&skill_file).unwrap();
        assert_ne!(content, "custom content", "should overwrite existing files");
    }
}
