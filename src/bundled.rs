/// Central registry of all files bundled with the ghost binary.
///
/// Every file that gets installed to `$WORKSPACE/` must be listed here.
/// This ensures all bundled files go through the same update-check code path.
use std::path::Path;

/// A file bundled into the ghost binary via `include_str!`.
pub struct BundledFile {
    /// Workspace-relative path, e.g. "skills/nix-shell/skill.md"
    pub path: &'static str,
    /// File content (from include_str!)
    pub content: &'static str,
}

/// Returns the complete list of files bundled with this binary.
///
/// Add new bundled files here — not in scattered install functions.
pub fn bundled_files() -> &'static [BundledFile] {
    BUNDLED_FILES
}

/// Install all bundled files to the workspace, always overwriting.
/// Creates parent directories as needed.
pub fn install_all(workspace: &Path) -> Result<(), std::io::Error> {
    for file in bundled_files() {
        let dest = workspace.join(file.path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, file.content)?;
    }
    save_manifest(workspace)?;
    Ok(())
}

/// Save the current bundled file paths as the manifest for future removal detection.
pub fn save_manifest(workspace: &Path) -> Result<(), std::io::Error> {
    let paths: Vec<&str> = bundled_files().iter().map(|f| f.path).collect();
    let json = serde_json::to_string_pretty(&paths).unwrap_or_default();
    let manifest_path = workspace.join(".cache/bundled-manifest.json");
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(manifest_path, json)
}

const BUNDLED_FILES: &[BundledFile] = &[
    // ── Workspace identity ──────────────────────────────────────────
    BundledFile {
        path: "BOOT.md",
        content: "# BOOT\n\nYou are a GHOST, a personal AI agent for your OPERATOR.\n",
    },
    // ── Shell ───────────────────────────────────────────────────────
    BundledFile {
        path: "shell/flake.nix",
        content: include_str!("../deploy/common/default-flake.nix"),
    },
    // ── Skills ──────────────────────────────────────────────────────
    // agent-creator
    BundledFile {
        path: "skills/agent-creator/skill.md",
        content: include_str!("../prompts/skills/agent-creator.md"),
    },
    // superpowers/brainstorming
    BundledFile {
        path: "skills/superpowers/brainstorming/skill.md",
        content: include_str!("../prompts/skills/superpowers/brainstorming/skill.md"),
    },
    // coding
    BundledFile {
        path: "skills/coding/skill.md",
        content: include_str!("../prompts/skills/coding.md"),
    },
    // deep-research (6 files)
    BundledFile {
        path: "skills/deep-research/skill.md",
        content: include_str!("../prompts/skills/deep-research/skill.md"),
    },
    BundledFile {
        path: "skills/deep-research/deep-research/agent.lua",
        content: include_str!("../prompts/skills/deep-research/deep-research/agent.lua"),
    },
    BundledFile {
        path: "skills/deep-research/deep-research/prompt.md",
        content: include_str!("../prompts/skills/deep-research/deep-research/prompt.md"),
    },
    BundledFile {
        path: "skills/deep-research/deep-research-reflection/agent.lua",
        content: include_str!("../prompts/skills/deep-research/deep-research-reflection/agent.lua"),
    },
    BundledFile {
        path: "skills/deep-research/deep-research-reflection/prompt.md",
        content: include_str!("../prompts/skills/deep-research/deep-research-reflection/prompt.md"),
    },
    BundledFile {
        path: "skills/deep-research/deep-research-reflection/user-message.md",
        content: include_str!(
            "../prompts/skills/deep-research/deep-research-reflection/user-message.md"
        ),
    },
    // superpowers/executing-plans
    BundledFile {
        path: "skills/superpowers/executing-plans/skill.md",
        content: include_str!("../prompts/skills/superpowers/executing-plans/skill.md"),
    },
    // superpowers/finishing-branch
    BundledFile {
        path: "skills/superpowers/finishing-branch/skill.md",
        content: include_str!("../prompts/skills/superpowers/finishing-branch/skill.md"),
    },
    // superpowers/git-worktrees
    BundledFile {
        path: "skills/superpowers/git-worktrees/skill.md",
        content: include_str!("../prompts/skills/superpowers/git-worktrees/skill.md"),
    },
    // image-generation (2 files)
    BundledFile {
        path: "skills/image-generation/skill.md",
        content: include_str!("../prompts/skills/image-generation/skill.md"),
    },
    BundledFile {
        path: "skills/image-generation/scripts/generate_image.py",
        content: include_str!("../prompts/skills/image-generation/scripts/generate_image.py"),
    },
    // knowledge-navigator (2 files)
    BundledFile {
        path: "skills/knowledge-navigator/skill.md",
        content: include_str!("../prompts/skills/knowledge-navigator/skill.md"),
    },
    BundledFile {
        path: "skills/knowledge-navigator/schema.sql",
        content: include_str!("../prompts/skills/knowledge-navigator/schema.sql"),
    },
    // nix-shell
    BundledFile {
        path: "skills/nix-shell/skill.md",
        content: include_str!("../prompts/skills/nix-shell.md"),
    },
    // note-writer
    BundledFile {
        path: "skills/note-writer/skill.md",
        content: include_str!("../prompts/skills/note-writer.md"),
    },
    // superpowers/parallel-agents
    BundledFile {
        path: "skills/superpowers/parallel-agents/skill.md",
        content: include_str!("../prompts/skills/superpowers/parallel-agents/skill.md"),
    },
    // project-manager
    BundledFile {
        path: "skills/project-manager/skill.md",
        content: include_str!("../prompts/skills/project-manager.md"),
    },
    // superpowers/receiving-review
    BundledFile {
        path: "skills/superpowers/receiving-review/skill.md",
        content: include_str!("../prompts/skills/superpowers/receiving-review/skill.md"),
    },
    // reference-import
    BundledFile {
        path: "skills/reference-import/skill.md",
        content: include_str!("../prompts/skills/reference-import.md"),
    },
    // sending-attachments
    BundledFile {
        path: "skills/sending-attachments/skill.md",
        content: include_str!("../prompts/skills/sending-attachments/skill.md"),
    },
    // superpowers/requesting-review (2 files)
    BundledFile {
        path: "skills/superpowers/requesting-review/skill.md",
        content: include_str!("../prompts/skills/superpowers/requesting-review/skill.md"),
    },
    BundledFile {
        path: "skills/superpowers/requesting-review/code-reviewer.md",
        content: include_str!("../prompts/skills/superpowers/requesting-review/code-reviewer.md"),
    },
    // superpowers/subagent-development (12 files)
    BundledFile {
        path: "skills/superpowers/subagent-development/skill.md",
        content: include_str!("../prompts/skills/superpowers/subagent-development/skill.md"),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/implementer-prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/implementer-prompt.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/spec-reviewer-prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/spec-reviewer-prompt.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/code-quality-reviewer-prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/code-quality-reviewer-prompt.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-implementer/agent.lua",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-implementer/agent.lua"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-implementer/prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-implementer/prompt.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-spec-reviewer/agent.lua",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-spec-reviewer/agent.lua"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-spec-reviewer/prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-spec-reviewer/prompt.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-quality-reviewer/agent.lua",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-quality-reviewer/agent.lua"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-quality-reviewer/prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-quality-reviewer/prompt.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-reviewer/agent.lua",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-reviewer/agent.lua"
        ),
    },
    BundledFile {
        path: "skills/superpowers/subagent-development/coding-reviewer/prompt.md",
        content: include_str!(
            "../prompts/skills/superpowers/subagent-development/coding-reviewer/prompt.md"
        ),
    },
    // superpowers/systematic-debugging (4 files)
    BundledFile {
        path: "skills/superpowers/systematic-debugging/skill.md",
        content: include_str!("../prompts/skills/superpowers/systematic-debugging/skill.md"),
    },
    BundledFile {
        path: "skills/superpowers/systematic-debugging/root-cause-tracing.md",
        content: include_str!(
            "../prompts/skills/superpowers/systematic-debugging/root-cause-tracing.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/systematic-debugging/condition-based-waiting.md",
        content: include_str!(
            "../prompts/skills/superpowers/systematic-debugging/condition-based-waiting.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/systematic-debugging/defense-in-depth.md",
        content: include_str!(
            "../prompts/skills/superpowers/systematic-debugging/defense-in-depth.md"
        ),
    },
    // superpowers/tdd (2 files)
    BundledFile {
        path: "skills/superpowers/tdd/skill.md",
        content: include_str!("../prompts/skills/superpowers/tdd/skill.md"),
    },
    BundledFile {
        path: "skills/superpowers/tdd/testing-anti-patterns.md",
        content: include_str!("../prompts/skills/superpowers/tdd/testing-anti-patterns.md"),
    },
    // superpowers/verification
    BundledFile {
        path: "skills/superpowers/verification/skill.md",
        content: include_str!("../prompts/skills/superpowers/verification/skill.md"),
    },
    // superpowers/writing-plans
    BundledFile {
        path: "skills/superpowers/writing-plans/skill.md",
        content: include_str!("../prompts/skills/superpowers/writing-plans/skill.md"),
    },
    // superpowers/writing-skills (4 files)
    BundledFile {
        path: "skills/superpowers/writing-skills/skill.md",
        content: include_str!("../prompts/skills/superpowers/writing-skills/skill.md"),
    },
    BundledFile {
        path: "skills/superpowers/writing-skills/best-practices.md",
        content: include_str!("../prompts/skills/superpowers/writing-skills/best-practices.md"),
    },
    BundledFile {
        path: "skills/superpowers/writing-skills/persuasion-principles.md",
        content: include_str!(
            "../prompts/skills/superpowers/writing-skills/persuasion-principles.md"
        ),
    },
    BundledFile {
        path: "skills/superpowers/writing-skills/testing-skills-with-subagents.md",
        content: include_str!(
            "../prompts/skills/superpowers/writing-skills/testing-skills-with-subagents.md"
        ),
    },
    // ── Agents ──────────────────────────────────────────────────────
    // chat-reflection (3 files)
    BundledFile {
        path: "agents/chat-reflection/agent.lua",
        content: include_str!("../prompts/agents/chat-reflection/agent.lua"),
    },
    BundledFile {
        path: "agents/chat-reflection/prompt.md",
        content: include_str!("../prompts/agents/chat-reflection/prompt.md"),
    },
    BundledFile {
        path: "agents/chat-reflection/user-message.md",
        content: include_str!("../prompts/agents/chat-reflection/user-message.md"),
    },
    // ── Agent infra ─────────────────────────────────────────────────
    BundledFile {
        path: "agents/.types/ghost.lua",
        content: include_str!("../prompts/types/ghost.lua"),
    },
    BundledFile {
        path: "agents/crontab.lua",
        content: include_str!("../prompts/agents/crontab.lua"),
    },
];
