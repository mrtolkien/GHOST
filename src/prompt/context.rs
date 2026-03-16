use std::path::Path;

/// Build system information: OS, hostname, current datetime, workspace path,
/// and available shell tools (parsed from `shell/flake.nix`).
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_system_info(workspace: &Path) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let hostname = gethostname::gethostname();
    let hostname = hostname.to_string_lossy();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M %Z");

    let mut info = format!(
        "OS: {os} ({arch})\n\
         Hostname: {hostname}\n\
         Date/Time: {now}\n\
         Workspace: {}",
        workspace.display()
    );

    if let Some(tools) = parse_flake_packages(&workspace.join("shell/flake.nix")) {
        info.push_str(&format!(
            "\nShell tools (via Nix — edit shell/flake.nix to add more): {tools}"
        ));
    }

    info
}

/// Extract package names from a Nix flake's `paths = with pkgs; [ ... ];`
/// or `packages = with pkgs; [ ... ];` block.
fn parse_flake_packages(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;

    let marker = if content.contains("paths = with pkgs; [") {
        "paths = with pkgs; ["
    } else if content.contains("packages = with pkgs; [") {
        "packages = with pkgs; ["
    } else {
        return None;
    };

    let start = content.find(marker)?;
    let after = &content[start..];
    let end = after.find(']')?;
    let block = &after[marker.len()..end];

    let names: Vec<&str> = block
        .split_whitespace()
        .filter(|s| !s.starts_with('#'))
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(names.join(", "))
}

/// Build model and provider information.
#[tracing::instrument(skip_all, level = "debug", fields(model, provider))]
pub fn build_model_info(model: &str, provider: &str) -> String {
    format!("Model: {model}\nProvider: {provider}")
}

/// Read BOOT.md and SOUL.md from the workspace, concatenating under headers.
/// Missing files produce empty sections (no error).
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_ghost_identity(workspace: &Path) -> String {
    let boot = read_optional_file(&workspace.join("BOOT.md"));
    let soul = read_optional_file(&workspace.join("SOUL.md"));

    let mut parts = Vec::new();

    if !boot.is_empty() {
        parts.push(boot);
    }
    if !soul.is_empty() {
        parts.push(format!("## Soul\n\n{soul}"));
    }

    parts.join("\n\n")
}

/// Read OPERATOR.md from the workspace. Missing file produces empty string.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_operator_context(workspace: &Path) -> String {
    read_optional_file(&workspace.join("OPERATOR.md"))
}

/// Scan the `skills/` directory, parse frontmatter, and list available
/// skills with descriptions in XML format (agentskills.io progressive
/// disclosure). Returns empty string if no skills found.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_ghost_skills(workspace: &Path) -> String {
    let skills: Vec<_> = crate::skills::discover_skills(workspace)
        .into_iter()
        .filter(|s| s.available.as_deref() != Some("coding"))
        .collect();

    if skills.is_empty() {
        return String::new();
    }

    let entries: Vec<String> = skills
        .iter()
        .map(|s| {
            let rel = s.path.strip_prefix(workspace).unwrap_or(&s.path).display();
            let source_tag = s
                .source
                .as_ref()
                .map(|src| format!("\n    <source>{src}</source>"))
                .unwrap_or_default();
            format!(
                "  <skill>\n    <name>{}</name>\n    \
                 <description>{}</description>\n    \
                 <location>{rel}</location>{source_tag}\n  </skill>",
                s.name, s.description,
            )
        })
        .collect();

    format!(
        "## Available Skills\n\n\
         <available_skills>\n{}\n</available_skills>",
        entries.join("\n"),
    )
}

/// Build recent diary entries for the system prompt.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_ghost_diary(workspace: &Path) -> String {
    let entries = crate::knowledge::load_recent_diary(workspace, 2);
    if entries.is_empty() {
        return String::new();
    }

    let mut parts = vec!["## Diary\n".to_string()];
    for (date, body) in &entries {
        parts.push(format!("### {date}\n\n{body}"));
    }
    parts.join("\n")
}

/// Scan `projects/` for active projects and build a summary section for the
/// system prompt. Returns empty string if no active projects exist.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_active_projects(workspace: &Path) -> String {
    let projects = match crate::projects::list_projects(workspace) {
        Ok(p) => p,
        Err(_) => return String::new(),
    };

    let active: Vec<_> = projects
        .iter()
        .filter(|(_, p)| p.front.status == crate::projects::ProjectStatus::Active)
        .collect();

    if active.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push("## Active Projects\n".to_string());

    for (slug, project) in &active {
        let (done, total) = crate::projects::task_summary(workspace, slug).unwrap_or((0, 0));
        lines.push(format!(
            "- **{}** (`{}`): {}/{} tasks done",
            project.front.title, slug, done, total
        ));
    }

    lines.push(String::new());
    lines.push(
        "Use `ghost project` commands to manage projects. \
         Read the `project-manager` skill for workflow guidance."
            .to_string(),
    );

    lines.join("\n")
}

fn read_optional_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ghost_identity_concatenates_boot_and_soul_under_headers() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("BOOT.md"), "# BOOT\nBe helpful.").unwrap();
        fs::write(dir.path().join("SOUL.md"), "I am Ghost.").unwrap();

        let identity = build_ghost_identity(dir.path());
        assert!(identity.contains("# BOOT"));
        assert!(identity.contains("Be helpful."));
        assert!(identity.contains("## Soul"));
        assert!(identity.contains("I am Ghost."));
    }

    #[test]
    fn ghost_identity_omits_soul_header_when_missing() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("BOOT.md"), "# BOOT\nDirectives").unwrap();

        let identity = build_ghost_identity(dir.path());
        assert!(identity.contains("Directives"));
        assert!(!identity.contains("Soul"));
    }

    #[test]
    fn skills_uses_xml_format_with_location() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");

        // Hidden dir with valid skill.md — should be skipped
        let hidden = skills.join(".hidden");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(
            hidden.join("skill.md"),
            "---\nname: hidden\ndescription: Hidden.\n---\n",
        )
        .unwrap();

        // Valid skills with frontmatter
        let nw = skills.join("note-writer");
        fs::create_dir_all(&nw).unwrap();
        fs::write(
            nw.join("skill.md"),
            "---\nname: note-writer\ndescription: Create notes.\n---\n",
        )
        .unwrap();

        let rs = skills.join("researcher");
        fs::create_dir_all(&rs).unwrap();
        fs::write(
            rs.join("skill.md"),
            "---\nname: researcher\ndescription: Research things.\n---\n",
        )
        .unwrap();

        let result = build_ghost_skills(dir.path());

        // XML structure
        assert!(result.contains("<available_skills>"));
        assert!(result.contains("<name>note-writer</name>"));
        assert!(result.contains("<description>Create notes.</description>"));
        assert!(result.contains("<location>skills/note-writer/skill.md</location>"));
        assert!(result.contains("<name>researcher</name>"));
        assert!(result.contains("<location>skills/researcher/skill.md</location>"));
        assert!(!result.contains("hidden"));

        // Sorted order
        let nw_pos = result.find("note-writer").unwrap();
        let rs_pos = result.find("researcher").unwrap();
        assert!(nw_pos < rs_pos);
    }

    #[test]
    fn system_info_includes_shell_tools_from_flake() {
        let dir = TempDir::new().unwrap();
        let shell_dir = dir.path().join("shell");
        fs::create_dir_all(&shell_dir).unwrap();
        fs::write(
            shell_dir.join("flake.nix"),
            "paths = with pkgs; [\n  git\n  ripgrep\n  jq\n];\n",
        )
        .unwrap();

        let info = build_system_info(dir.path());
        assert!(info.contains("git, ripgrep, jq"));
        assert!(info.contains("shell/flake.nix"));
    }

    #[test]
    fn system_info_omits_shell_tools_when_no_flake() {
        let dir = TempDir::new().unwrap();
        let info = build_system_info(dir.path());
        assert!(!info.contains("Shell tools"));
    }

    #[test]
    fn active_projects_empty_when_no_projects() {
        let dir = TempDir::new().unwrap();
        let result = build_active_projects(dir.path());
        assert!(result.is_empty());
    }

    #[test]
    fn active_projects_shows_active_only() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("projects")).unwrap();

        // Create an active project with a task
        crate::projects::init_project(dir.path(), "Active One", &[]).unwrap();
        crate::projects::create_task(dir.path(), "active_one", "Task A", &[], "").unwrap();

        // Create a paused project
        let (slug, _) = crate::projects::init_project(dir.path(), "Paused One", &[]).unwrap();
        let mut p = crate::projects::read_project(dir.path(), &slug).unwrap();
        p.front.status = crate::projects::ProjectStatus::Paused;
        crate::projects::write_project(dir.path(), &slug, &p.front, &p.body).unwrap();

        let result = build_active_projects(dir.path());
        assert!(result.contains("Active One"));
        assert!(result.contains("0/1 tasks done"));
        assert!(!result.contains("Paused One"));
        assert!(result.contains("ghost project"));
    }

    #[test]
    fn build_ghost_skills_excludes_coding_only() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");

        // Coding-only skill
        let coding = skills.join("coding-skill");
        fs::create_dir_all(&coding).unwrap();
        fs::write(
            coding.join("skill.md"),
            "---\nname: coding-skill\ndescription: Coding only.\navailable: coding\n---\n",
        )
        .unwrap();

        // General skill
        let general = skills.join("general-skill");
        fs::create_dir_all(&general).unwrap();
        fs::write(
            general.join("skill.md"),
            "---\nname: general-skill\ndescription: For everyone.\n---\n",
        )
        .unwrap();

        let result = build_ghost_skills(dir.path());
        assert!(result.contains("general-skill"));
        assert!(!result.contains("coding-skill"));
    }

    #[test]
    fn skills_includes_source_when_present() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        let s = skills.join("remote-skill");
        fs::create_dir_all(&s).unwrap();
        fs::write(
            s.join("skill.md"),
            "---\nname: remote-skill\ndescription: Remote.\nsource: https://example.com/skill\n---\n",
        )
        .unwrap();

        let result = build_ghost_skills(dir.path());
        assert!(result.contains("<source>https://example.com/skill</source>"));
    }

    #[test]
    fn skills_omits_source_when_absent() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        let s = skills.join("local-skill");
        fs::create_dir_all(&s).unwrap();
        fs::write(
            s.join("skill.md"),
            "---\nname: local-skill\ndescription: Local.\n---\n",
        )
        .unwrap();

        let result = build_ghost_skills(dir.path());
        assert!(!result.contains("<source>"));
    }

    #[test]
    fn build_ghost_diary_formats_recent_entries() {
        let dir = TempDir::new().unwrap();
        let diary_dir = dir.path().join("diary");
        fs::create_dir_all(&diary_dir).unwrap();
        fs::write(diary_dir.join("2026-03-07.md"), "Had a great chat.").unwrap();
        fs::write(diary_dir.join("2026-03-08.md"), "Built a feature.").unwrap();

        let result = build_ghost_diary(dir.path());
        assert!(result.contains("## Diary"));
        assert!(result.contains("### 2026-03-07"));
        assert!(result.contains("Had a great chat."));
        assert!(result.contains("### 2026-03-08"));
        assert!(result.contains("Built a feature."));
    }

    #[test]
    fn build_ghost_diary_empty_when_no_entries() {
        let dir = TempDir::new().unwrap();
        let result = build_ghost_diary(dir.path());
        assert!(result.is_empty());
    }
}
