use std::fs;
use std::path::Path;

/// Build system information: OS, hostname, current datetime, workspace path.
#[tracing::instrument(skip_all, fields(workspace = %workspace.display()))]
pub fn build_system_info(workspace: &Path) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let hostname = gethostname::gethostname();
    let hostname = hostname.to_string_lossy();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M %Z");

    format!(
        "OS: {os} ({arch})\n\
         Hostname: {hostname}\n\
         Date/Time: {now}\n\
         Workspace: {}",
        workspace.display()
    )
}

/// Build model and provider information.
#[tracing::instrument(skip_all, fields(model, provider))]
pub fn build_model_info(model: &str, provider: &str) -> String {
    format!("Model: {model}\nProvider: {provider}")
}

/// Read BOOT.md and SOUL.md from the workspace, concatenating under headers.
/// Missing files produce empty sections (no error).
#[tracing::instrument(skip_all, fields(workspace = %workspace.display()))]
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
#[tracing::instrument(skip_all, fields(workspace = %workspace.display()))]
pub fn build_operator_context(workspace: &Path) -> String {
    read_optional_file(&workspace.join("OPERATOR.md"))
}

/// Scan the `skills/` directory and list available skills.
/// Returns empty string if the directory doesn't exist or is empty.
#[tracing::instrument(skip_all, fields(workspace = %workspace.display()))]
pub fn build_ghost_skills(workspace: &Path) -> String {
    let skills_dir = workspace.join("skills");

    let entries = match fs::read_dir(&skills_dir) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };

    let mut skill_names: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden files/dirs
        if name.starts_with('.') {
            continue;
        }
        skill_names.push(name);
    }

    if skill_names.is_empty() {
        return String::new();
    }

    skill_names.sort();

    let list = skill_names
        .iter()
        .map(|n| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Available skills:\n{list}")
}

/// Placeholder for diary content. Will be wired to SurrealDB in spec 15.
#[tracing::instrument(skip_all)]
pub fn build_ghost_diary() -> String {
    // TODO(spec-15): wire to SurrealDB diary query
    String::new()
}

/// Placeholder for available CLI commands.
#[tracing::instrument(skip_all)]
pub fn build_ghost_commands() -> String {
    String::new()
}

fn read_optional_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
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
    fn skills_lists_sorted_entries_and_skips_hidden() {
        let dir = TempDir::new().unwrap();
        let skills = dir.path().join("skills");
        fs::create_dir(&skills).unwrap();
        fs::create_dir(skills.join(".hidden")).unwrap();
        fs::create_dir(skills.join("note-writer")).unwrap();
        fs::create_dir(skills.join("researcher")).unwrap();

        let result = build_ghost_skills(dir.path());
        assert!(!result.contains(".hidden"));
        assert!(result.contains("- note-writer"));
        assert!(result.contains("- researcher"));
        // Verify sorted order
        let nw = result.find("note-writer").unwrap();
        let rs = result.find("researcher").unwrap();
        assert!(nw < rs);
    }
}
