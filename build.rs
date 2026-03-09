use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("bundled_files.rs");
    let assets_dir = Path::new("assets");

    println!("cargo::rerun-if-changed=assets");

    let mut entries = Vec::new();
    walk_dir(assets_dir, assets_dir, &mut entries);
    entries.sort();

    let mut f = fs::File::create(&dest).unwrap();

    writeln!(f, "const BUNDLED_FILES: &[BundledFile] = &[").unwrap();
    for (workspace_path, source_path) in &entries {
        writeln!(f, "    BundledFile {{").unwrap();
        writeln!(f, "        path: {workspace_path:?},").unwrap();
        writeln!(f, "        content: include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{source_path}\")),").unwrap();
        writeln!(f, "    }},").unwrap();
    }
    writeln!(f, "];").unwrap();
}

/// Recursively walk a directory, collecting (workspace_path, source_path) pairs.
fn walk_dir(base: &Path, dir: &Path, entries: &mut Vec<(String, String)>) {
    let mut children: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .collect();
    children.sort_by_key(|e| e.file_name());

    for entry in children {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(base, &path, entries);
        } else {
            let rel = path.strip_prefix(base).unwrap();
            let workspace_path = rel.to_string_lossy().to_string();
            let source_path = path.to_string_lossy().to_string();
            entries.push((workspace_path, source_path));
        }
    }
}
