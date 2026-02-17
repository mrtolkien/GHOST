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

/// Scan the `skills/` directory, parse frontmatter, and list available
/// skills with descriptions. Returns empty string if no skills found.
#[tracing::instrument(skip_all, level = "debug", fields(workspace = %workspace.display()))]
pub fn build_ghost_skills(workspace: &Path) -> String {
    let skills = crate::skills::discover_skills(workspace);

    if skills.is_empty() {
        return String::new();
    }

    let list = skills
        .iter()
        .map(|s| format!("- **{}** — {}", s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "## Available Skills\n\n\
         Use `read_file` to load the full instructions for any \
         skill before using it.\n\n\
         {list}"
    )
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
    fn skills_lists_sorted_entries_with_descriptions_and_skips_hidden() {
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

        assert!(!result.contains("hidden"));
        assert!(result.contains("**note-writer** — Create notes."));
        assert!(result.contains("**researcher** — Research things."));
        assert!(result.contains("## Available Skills"));

        // Verify sorted order
        let nw_pos = result.find("note-writer").unwrap();
        let rs_pos = result.find("researcher").unwrap();
        assert!(nw_pos < rs_pos);
    }
}
