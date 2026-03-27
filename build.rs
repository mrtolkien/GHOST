#![allow(
    clippy::unwrap_used,
    reason = "build script — panic on failure is the correct behavior"
)]

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("bundled_files.rs");

    // Bundle assets/
    let assets_dir = Path::new("assets");
    println!("cargo::rerun-if-changed=assets");

    let mut entries = Vec::new();
    walk_dir(assets_dir, assets_dir, &mut entries, &[]);
    // All assets are bundled to $WORKSPACE/.
    entries.sort();

    let mut f = fs::File::create(&dest).unwrap();
    write_array(&mut f, "BUNDLED_FILES", &entries);

    // Bundle docs/src/content/ (self-documentation for references/)
    let docs_dir = Path::new("docs/src/content");
    println!("cargo::rerun-if-changed=docs/src/content");

    let mut doc_entries = Vec::new();
    walk_dir(docs_dir, docs_dir, &mut doc_entries, &[]);
    doc_entries.sort();

    write_array(&mut f, "BUNDLED_DOCS", &doc_entries);

    // --- Git commit hash for `ghost version` ---
    // In Nix builds, .git is absent — the flake passes GIT_COMMIT_HASH via env.
    // In dev builds, we read it from git directly.
    let git_hash = std::env::var("GIT_COMMIT_HASH").ok().unwrap_or_else(|| {
        // Track git state for rebuild triggers (dev builds only)
        println!("cargo::rerun-if-changed=.git/HEAD");
        if let Ok(head) = std::fs::read_to_string(".git/HEAD")
            && let Some(ref_path) = head.strip_prefix("ref: ")
        {
            println!("cargo::rerun-if-changed=.git/{}", ref_path.trim());
        }

        std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "unknown".into())
    });
    println!("cargo::rustc-env=GIT_COMMIT_HASH={git_hash}");
}

fn write_array(f: &mut fs::File, name: &str, entries: &[(String, String)]) {
    writeln!(f, "const {name}: &[BundledFile] = &[").unwrap();
    for (workspace_path, source_path) in entries {
        writeln!(f, "    BundledFile {{").unwrap();
        writeln!(f, "        path: {workspace_path:?},").unwrap();
        writeln!(
            f,
            "        content: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{source_path}\")),",
        )
        .unwrap();
        writeln!(f, "    }},").unwrap();
    }
    writeln!(f, "];").unwrap();
}

/// Recursively walk a directory, collecting (workspace_path, source_path) pairs.
/// `skip_dirs` contains top-level directory names to exclude (e.g. "services").
fn walk_dir(base: &Path, dir: &Path, entries: &mut Vec<(String, String)>, skip_dirs: &[&str]) {
    // Gracefully handle missing directories (e.g. during buildDepsOnly where
    // only Cargo sources are present — assets/ and docs/ won't exist).
    let mut children: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(std::result::Result::ok).collect(),
        Err(_) => return,
    };
    children.sort_by_key(std::fs::DirEntry::file_name);

    for entry in children {
        let path = entry.path();
        if path.is_dir() {
            // Skip excluded top-level directories
            if dir == base {
                let name = entry.file_name();
                if skip_dirs.iter().any(|s| *s == name.to_string_lossy()) {
                    continue;
                }
            }
            walk_dir(base, &path, entries, skip_dirs);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            let workspace_path = rel.to_string_lossy().to_string();
            let source_path = path.to_string_lossy().to_string();
            entries.push((workspace_path, source_path));
        }
    }
}
