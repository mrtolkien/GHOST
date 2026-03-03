use tree_sitter::{Language, Node, Parser};

use super::chunker::Chunk;

/// Target size for each code chunk in characters.
const CHUNK_TARGET: usize = 1000;

/// Attempt to chunk source code using tree-sitter AST analysis.
///
/// Returns `None` if the file extension is not recognized, allowing the
/// caller to fall back to plain text chunking. Returns `Some(chunks)` with
/// AST-aware chunks that include file/language/scope metadata headers.
pub fn chunk_code(content: &str, file_path: &str, tags: &[String]) -> Option<Vec<Chunk>> {
    let (language, lang_name) = detect_language(file_path)?;

    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut raw_chunks: Vec<String> = Vec::new();
    collect_chunks(root, content, &mut raw_chunks);

    if raw_chunks.is_empty() {
        return None;
    }

    // Greedy merge: pack small consecutive chunks up to CHUNK_TARGET
    let merged = greedy_merge(&raw_chunks);

    let tag_prefix = if tags.is_empty() {
        String::new()
    } else {
        format!("[tags: {}] ", tags.join(", "))
    };

    let chunks = merged
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let header = if i == 0 {
                format!("{tag_prefix}[file: {file_path}] [language: {lang_name}]\n")
            } else {
                format!("[file: {file_path}] [language: {lang_name}]\n")
            };
            Chunk {
                index: i,
                text: format!("{header}{text}"),
            }
        })
        .collect();

    Some(chunks)
}

/// Recursively collect chunks from the AST. If a node's text fits in
/// `CHUNK_TARGET`, emit it as a chunk. If oversized, recurse into children.
fn collect_chunks<'a>(node: Node<'a>, source: &str, out: &mut Vec<String>) {
    let text = &source[node.byte_range()];
    if text.len() <= CHUNK_TARGET {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        return;
    }

    // Oversized — try to break into children
    let child_count = node.child_count();
    if child_count == 0 {
        // Leaf node that's too big — emit as-is (will be merged/truncated)
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            out.push(trimmed.to_string());
        }
        return;
    }

    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            collect_chunks(child, source, out);
        }
    }
}

/// Greedily merge consecutive small chunks up to CHUNK_TARGET.
fn greedy_merge(chunks: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    let mut current = String::new();

    for chunk in chunks {
        if current.is_empty() {
            current = chunk.clone();
        } else if current.len() + chunk.len() < CHUNK_TARGET {
            current.push('\n');
            current.push_str(chunk);
        } else {
            merged.push(std::mem::take(&mut current));
            current = chunk.clone();
        }
    }

    if !current.is_empty() {
        merged.push(current);
    }

    merged
}

/// Detect programming language from file extension.
/// Returns the tree-sitter Language and a human-readable name.
fn detect_language(file_path: &str) -> Option<(Language, &'static str)> {
    let ext = file_path.rsplit('.').next()?;
    match ext {
        "rs" => Some((tree_sitter_rust::LANGUAGE.into(), "rust")),
        "py" | "pyi" => Some((tree_sitter_python::LANGUAGE.into(), "python")),
        "js" | "mjs" | "cjs" => Some((tree_sitter_javascript::LANGUAGE.into(), "javascript")),
        "ts" => Some((
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
        )),
        "tsx" => Some((tree_sitter_typescript::LANGUAGE_TSX.into(), "tsx")),
        "go" => Some((tree_sitter_go::LANGUAGE.into(), "go")),
        "sh" | "bash" => Some((tree_sitter_bash::LANGUAGE.into(), "bash")),
        "toml" => Some((tree_sitter_toml_ng::LANGUAGE.into(), "toml")),
        "json" => Some((tree_sitter_json::LANGUAGE.into(), "json")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_rust_code() {
        let code = r#"
fn hello() {
    println!("hello");
}

fn world() {
    println!("world");
}
"#;
        let chunks = chunk_code(code, "src/main.rs", &[]).expect("should detect rust");
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("[file: src/main.rs]"));
        assert!(chunks[0].text.contains("[language: rust]"));
    }

    #[test]
    fn returns_none_for_unknown_extension() {
        let result = chunk_code("hello", "data.csv", &[]);
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_for_markdown() {
        let result = chunk_code("# Hello\nworld", "docs/readme.md", &[]);
        assert!(result.is_none());
    }

    #[test]
    fn includes_tags_on_first_chunk() {
        let code = "fn main() {}";
        let chunks =
            chunk_code(code, "src/main.rs", &["dioxus".into()]).expect("should detect rust");
        assert!(chunks[0].text.contains("[tags: dioxus]"));
    }

    #[test]
    fn splits_large_file() {
        // Generate a file large enough to require splitting
        let functions: Vec<String> = (0..30)
            .map(|i| {
                format!(
                    "fn func_{i}() {{\n    let x = {i};\n    println!(\"{{x}}\");\n    \
                     let y = x * 2;\n    println!(\"{{y}}\");\n}}\n"
                )
            })
            .collect();
        let code = functions.join("\n");
        let chunks = chunk_code(&code, "src/big.rs", &[]).expect("should detect rust");
        assert!(
            chunks.len() > 1,
            "large file should produce multiple chunks, got {}",
            chunks.len()
        );
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn chunks_python() {
        let code = "def hello():\n    print('hello')\n";
        let chunks = chunk_code(code, "app.py", &[]).expect("should detect python");
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("[language: python]"));
    }

    #[test]
    fn chunks_typescript() {
        let code = "function greet(name: string): void {\n  console.log(name);\n}\n";
        let chunks = chunk_code(code, "app.ts", &[]).expect("should detect typescript");
        assert!(!chunks.is_empty());
        assert!(chunks[0].text.contains("[language: typescript]"));
    }
}
