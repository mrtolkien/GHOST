use std::path::{Path, PathBuf};

use url::Url;

/// Maximum length for generated slugs (characters).
const MAX_SLUG_LEN: usize = 60;
const MAX_STAGING_SUFFIX: u32 = 999;

/// Derive a short slug from a source identifier (URL or file path).
///
/// - Git URLs: `owner-repo` (strips `.git` suffix).
/// - HTTP URLs with path segments: domain only.
/// - File paths: file stem.
#[must_use]
pub fn slug_from_source(source: &str) -> String {
    if let Ok(url) = Url::parse(source) {
        slug_from_url(&url)
    } else {
        slug_from_path(source)
    }
}

/// Extract a slug from a parsed URL.
///
/// Git repo URLs produce `owner-repo`; other HTTP URLs produce the domain.
fn slug_from_url(url: &Url) -> String {
    let domain = url.host_str().unwrap_or("unknown");
    let path = url.path().trim_matches('/');

    // Recognized as a git repo if hosted on a known forge OR path ends with .git
    let looks_like_git = is_git_forge(domain) || path.ends_with(".git");
    if looks_like_git
        && let Some(slug) = try_git_slug(path)
    {
        return slugify(&slug);
    }

    // Non-git URL or git forge with unusual path: use domain only.
    slugify(domain)
}

/// Known Git forge domains. A URL must be hosted on one of these to be
/// treated as a `owner/repo` Git source.
const GIT_FORGE_DOMAINS: &[&str] = &[
    "github.com",
    "gitlab.com",
    "codeberg.org",
    "bitbucket.org",
    "sr.ht",
    "gitea.com",
];

/// Check whether a domain belongs to a known Git forge.
fn is_git_forge(domain: &str) -> bool {
    let d = domain.strip_prefix("www.").unwrap_or(domain);
    GIT_FORGE_DOMAINS.iter().any(|forge| d.eq_ignore_ascii_case(forge))
}

/// Try to extract `owner-repo` from a URL path that looks like a Git repo.
///
/// Returns `Some("owner-repo")` when the path has exactly the shape
/// `owner/repo[.git]` (two leading segments), which covers GitHub, GitLab,
/// Codeberg, etc.
fn try_git_slug(path: &str) -> Option<String> {
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // A bare `owner/repo` or `owner/repo.git` has exactly 2 segments.
    if segments.len() != 2 {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1].trim_end_matches(".git");

    // Both must be non-empty after trimming.
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    Some(format!("{owner}-{repo}"))
}

/// Extract a slug from a file path (uses the file stem).
fn slug_from_path(source: &str) -> String {
    let path = Path::new(source);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    slugify(stem)
}

/// Create a staging directory under `staging_root` named after `slug`.
///
/// If the directory already exists, appends a numeric suffix (`-2`, `-3`, ...)
/// until a unique name is found.
pub fn create_staging_dir(
    staging_root: &Path,
    slug: &str,
) -> Result<PathBuf, std::io::Error> {
    let candidate = staging_root.join(slug);
    if !candidate.exists() {
        std::fs::create_dir_all(&candidate)?;
        return Ok(candidate);
    }

    for suffix in 2..=MAX_STAGING_SUFFIX {
        let name = format!("{slug}-{suffix}");
        let candidate = staging_root.join(&name);
        if !candidate.exists() {
            std::fs::create_dir_all(&candidate)?;
            return Ok(candidate);
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("too many staging dirs for slug '{slug}'"),
    ))
}

/// Lowercase, replace non-alphanumeric with hyphens, collapse runs, trim
/// leading/trailing hyphens, and truncate to [`MAX_SLUG_LEN`] characters.
fn slugify(s: &str) -> String {
    let raw: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    let mut result = String::with_capacity(raw.len());
    let mut prev_dash = true; // start true to trim leading hyphens
    for c in raw.chars() {
        if c == '-' {
            if !prev_dash {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
            prev_dash = false;
        }
    }

    let trimmed = result.trim_end_matches('-');
    if trimmed.len() > MAX_SLUG_LEN {
        // All chars are ASCII so byte boundary == char boundary, but be safe.
        let mut end = MAX_SLUG_LEN;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed[..end].trim_end_matches('-').to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // --- slugify ---

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_collapses_runs() {
        assert_eq!(slugify("a---b...c"), "a-b-c");
    }

    #[test]
    fn slugify_trims_leading_and_trailing() {
        assert_eq!(slugify("--hello--"), "hello");
    }

    #[test]
    fn slugify_empty_string() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_all_special_chars() {
        assert_eq!(slugify("!!!@@@###"), "");
    }

    #[test]
    fn slugify_truncates_long_input() {
        let long = "a".repeat(100);
        let slug = slugify(&long);
        assert!(slug.len() <= MAX_SLUG_LEN);
        assert_eq!(slug, "a".repeat(MAX_SLUG_LEN));
    }

    #[test]
    fn slugify_preserves_digits() {
        assert_eq!(slugify("v2.0-beta.3"), "v2-0-beta-3");
    }

    // --- slug_from_source: Git URLs ---

    #[test]
    fn slug_from_source_github_https() {
        assert_eq!(
            slug_from_source("https://github.com/DioxusLabs/docsite"),
            "dioxuslabs-docsite"
        );
    }

    #[test]
    fn slug_from_source_github_with_git_suffix() {
        assert_eq!(
            slug_from_source("https://github.com/owner/repo.git"),
            "owner-repo"
        );
    }

    #[test]
    fn slug_from_source_gitlab() {
        assert_eq!(
            slug_from_source("https://gitlab.com/AcmeCorp/my-project.git"),
            "acmecorp-my-project"
        );
    }

    // --- slug_from_source: HTTP URLs (non-git) ---

    #[test]
    fn slug_from_source_generic_url() {
        assert_eq!(
            slug_from_source("https://ghost.tolki.dev/docs/getting-started"),
            "ghost-tolki-dev"
        );
    }

    #[test]
    fn slug_from_source_url_with_www() {
        assert_eq!(
            slug_from_source("https://www.example.com/page"),
            "www-example-com"
        );
    }

    // --- slug_from_source: file paths ---

    #[test]
    fn slug_from_source_absolute_path() {
        assert_eq!(
            slug_from_source("/home/user/docs/quarterly-report.pdf"),
            "quarterly-report"
        );
    }

    #[test]
    fn slug_from_source_relative_path() {
        assert_eq!(slug_from_source("notes/my_file.md"), "my-file");
    }

    #[test]
    fn slug_from_source_filename_only() {
        assert_eq!(slug_from_source("README.md"), "readme");
    }

    // --- slug_from_source: edge cases ---

    #[test]
    fn slug_from_source_deep_github_path_is_not_git() {
        // More than 2 segments — not a simple owner/repo, so falls back to domain.
        assert_eq!(
            slug_from_source("https://github.com/owner/repo/tree/main/docs"),
            "github-com"
        );
    }

    // --- create_staging_dir ---

    #[test]
    fn create_staging_dir_basic() {
        let tmp = TempDir::new().unwrap();
        let dir = create_staging_dir(tmp.path(), "test-slug").unwrap();
        assert_eq!(dir.file_name().unwrap(), "test-slug");
        assert!(dir.is_dir());
    }

    #[test]
    fn create_staging_dir_deduplicates() {
        let tmp = TempDir::new().unwrap();

        let first = create_staging_dir(tmp.path(), "dup").unwrap();
        assert_eq!(first.file_name().unwrap(), "dup");

        let second = create_staging_dir(tmp.path(), "dup").unwrap();
        assert_eq!(second.file_name().unwrap(), "dup-2");

        let third = create_staging_dir(tmp.path(), "dup").unwrap();
        assert_eq!(third.file_name().unwrap(), "dup-3");
    }

    #[test]
    fn create_staging_dir_creates_parents() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b");
        // Root doesn't exist yet — create_dir_all should handle it.
        let dir = create_staging_dir(&nested, "slug").unwrap();
        assert!(dir.is_dir());
    }

    #[test]
    fn create_staging_dir_skips_file_collision() {
        let tmp = TempDir::new().unwrap();
        // Place a regular file where the first dir would go.
        let blocker = tmp.path().join("blocked");
        fs::write(&blocker, "not a directory").unwrap();

        let dir = create_staging_dir(tmp.path(), "blocked").unwrap();
        assert_eq!(dir.file_name().unwrap(), "blocked-2");
        assert!(dir.is_dir());
    }
}
