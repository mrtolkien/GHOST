use std::path::Path;

use super::extract_wiki_links;

/// Strip `[[references/...]]` wiki links that point to non-existent files.
///
/// Regular wiki links (e.g. `[[Bambu Lab]]`) are fine — those create stubs.
/// But `[[references/topic/filename]]` pointing to a non-existent file would
/// be a broken citation. We strip these and return a warning message.
pub fn sanitize_reference_links(workspace: &Path, body: &str) -> (String, Option<String>) {
    let links = extract_wiki_links(body);
    let missing: Vec<&str> = links
        .iter()
        .filter(|link| link.target.starts_with("references/"))
        .filter(|link| {
            let path = workspace.join(&link.target);
            let path_md = workspace.join(format!("{}.md", link.target));
            !path.exists() && !path_md.exists()
        })
        .map(|link| link.target.as_str())
        .collect();

    if missing.is_empty() {
        return (body.to_string(), None);
    }

    // Strip the broken reference links from the body
    let mut sanitized = body.to_string();
    for target in &missing {
        // Remove [[references/...]] patterns — try with relationship prefix too
        let plain = format!("[[{target}]]");
        sanitized = sanitized.replace(&plain, "");
        // Also handle [[rel>references/...]] patterns
        for prefix in &["source>", "from>", "cited_in>"] {
            let with_rel = format!("[[{prefix}{target}]]");
            sanitized = sanitized.replace(&with_rel, "");
        }
    }

    let warning = format!(
        "Stripped {} broken reference link(s) — the referenced file(s) do not exist. \
         Review the URL or slug.\n\
         Removed:\n{}",
        missing.len(),
        missing
            .iter()
            .map(|p| format!("  - [[{p}]]"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    (sanitized, Some(warning))
}
