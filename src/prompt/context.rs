use std::fs;
use std::path::Path;

/// Build system information: OS, hostname, current datetime, workspace path.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
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

/// Scan the `skills/` directory and list available skills.
/// Returns empty string if the directory doesn't exist or is empty.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
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
#[tracing::instrument(skip_all, level = "debug")]
pub fn build_ghost_diary() -> String {
    // TODO(spec-15): wire to SurrealDB diary query
    String::new()
}

/// CLI commands available via the shell tool.
#[tracing::instrument(skip_all, level = "debug")]
pub fn build_ghost_commands() -> String {
    r#"## Available Commands

### Web Search
```
ghost web search "<query>" [-n <max_results>]
```
Search the web using Brave Search. Returns numbered results with title, URL, and
snippet. Results are auto-cached to `.web-cache/` for later curation.

### Web Fetch
```
ghost web fetch "<url>" [--max-chars <N>] [--readability] [--raw]
```
Fetch a URL and convert it to Markdown. Output goes to stdout; cache path to stderr.

**Choosing the right mode:**

- **Default** (no flags): converts full HTML to Markdown. All page content is
  preserved — headings, links, lists, navigation, sidebars. Use this for:
  - Documentation pages, API references
  - Index/listing pages, homepages
  - Search result pages, forums
  - Any page where you need the complete content
- **`--readability`**: extracts only the main article body, stripping navigation,
  sidebars, headers, footers, and boilerplate. Use this for:
  - Blog posts and news articles
  - Essays, tutorials, long-form writing
  - Any page with a single primary article you want to read cleanly
- **`--raw`**: returns raw HTML with no conversion. Use when Markdown conversion
  loses important structural information or you need to inspect the page source.

Options:
- `--max-chars <N>`: truncate output at N characters (default 50000)

All results are auto-cached to `$WORKSPACE/.web-cache/`."#
        .to_string()
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
