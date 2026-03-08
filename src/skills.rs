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
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/deep-research/skill.md"),
            ),
            (
                "deep-research/agent.lua",
                include_str!("../prompts/skills/deep-research/deep-research/agent.lua"),
            ),
            (
                "deep-research/prompt.md",
                include_str!("../prompts/skills/deep-research/deep-research/prompt.md"),
            ),
            (
                "deep-research-reflection/agent.lua",
                include_str!("../prompts/skills/deep-research/deep-research-reflection/agent.lua"),
            ),
            (
                "deep-research-reflection/prompt.md",
                include_str!("../prompts/skills/deep-research/deep-research-reflection/prompt.md"),
            ),
            (
                "deep-research-reflection/user-message.md",
                include_str!(
                    "../prompts/skills/deep-research/deep-research-reflection/user-message.md"
                ),
            ),
        ],
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
        path: "image-generation",
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/image-generation/skill.md"),
            ),
            (
                "scripts/generate_image.py",
                include_str!("../prompts/skills/image-generation/scripts/generate_image.py"),
            ),
        ],
    },
    DefaultSkill {
        path: "knowledge-navigator",
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/knowledge-navigator/skill.md"),
            ),
            (
                "schema.sql",
                include_str!("../prompts/skills/knowledge-navigator/schema.sql"),
            ),
        ],
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
        path: "sending-attachments",
        files: &[(
            "skill.md",
            include_str!("../prompts/skills/sending-attachments/skill.md"),
        )],
    },
    DefaultSkill {
        path: "superpowers/requesting-review",
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/superpowers/requesting-review/skill.md"),
            ),
            (
                "code-reviewer.md",
                include_str!("../prompts/skills/superpowers/requesting-review/code-reviewer.md"),
            ),
        ],
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
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/implementer-prompt.md"
                ),
            ),
            (
                "spec-reviewer-prompt.md",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/spec-reviewer-prompt.md"
                ),
            ),
            (
                "code-quality-reviewer-prompt.md",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/code-quality-reviewer-prompt.md"
                ),
            ),
            (
                "coding-implementer/agent.lua",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-implementer/agent.lua"
                ),
            ),
            (
                "coding-implementer/prompt.md",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-implementer/prompt.md"
                ),
            ),
            (
                "coding-spec-reviewer/agent.lua",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-spec-reviewer/agent.lua"
                ),
            ),
            (
                "coding-spec-reviewer/prompt.md",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-spec-reviewer/prompt.md"
                ),
            ),
            (
                "coding-quality-reviewer/agent.lua",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-quality-reviewer/agent.lua"
                ),
            ),
            (
                "coding-quality-reviewer/prompt.md",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-quality-reviewer/prompt.md"
                ),
            ),
            (
                "coding-reviewer/agent.lua",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-reviewer/agent.lua"
                ),
            ),
            (
                "coding-reviewer/prompt.md",
                include_str!(
                    "../prompts/skills/superpowers/subagent-development/coding-reviewer/prompt.md"
                ),
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
                include_str!(
                    "../prompts/skills/superpowers/systematic-debugging/root-cause-tracing.md"
                ),
            ),
            (
                "condition-based-waiting.md",
                include_str!(
                    "../prompts/skills/superpowers/systematic-debugging/condition-based-waiting.md"
                ),
            ),
            (
                "defense-in-depth.md",
                include_str!(
                    "../prompts/skills/superpowers/systematic-debugging/defense-in-depth.md"
                ),
            ),
        ],
    },
    DefaultSkill {
        path: "superpowers/tdd",
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/superpowers/tdd/skill.md"),
            ),
            (
                "testing-anti-patterns.md",
                include_str!("../prompts/skills/superpowers/tdd/testing-anti-patterns.md"),
            ),
        ],
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
        files: &[
            (
                "skill.md",
                include_str!("../prompts/skills/superpowers/writing-skills/skill.md"),
            ),
            (
                "best-practices.md",
                include_str!("../prompts/skills/superpowers/writing-skills/best-practices.md"),
            ),
            (
                "persuasion-principles.md",
                include_str!(
                    "../prompts/skills/superpowers/writing-skills/persuasion-principles.md"
                ),
            ),
            (
                "testing-skills-with-subagents.md",
                include_str!(
                    "../prompts/skills/superpowers/writing-skills/testing-skills-with-subagents.md"
                ),
            ),
        ],
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
            let file_path = skill_dir.join(filename);
            // Agent files live in subdirectories (e.g. "deep-research/agent.lua")
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(file_path, content)?;
        }
    }

    Ok(())
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
                if *filename == "skill.md" {
                    assert!(
                        content.contains("---"),
                        "skill.md in {} should have frontmatter",
                        skill.path
                    );
                }
                assert!(!content.is_empty(), "File {file:?} should not be empty");
            }
        }

        assert_eq!(DEFAULT_SKILLS.len(), 23);
    }

    #[test]
    fn install_default_skills_creates_agent_subdirs() {
        let dir = TempDir::new().unwrap();
        install_default_skills(dir.path()).unwrap();

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
