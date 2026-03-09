/// Result of a tool execution. Most tools return text-only output.
/// Tools like `read_file` can include images alongside text.
#[derive(Debug)]
pub struct ToolOutput {
    pub text: String,
    pub images: Vec<ImageRef>,
}

#[derive(Debug)]
pub struct ImageRef {
    pub path: String,
    pub mime_type: String,
    pub filename: String,
}

impl ToolOutput {
    /// Create a text-only output (most tools).
    pub fn text(text: String) -> Self {
        Self {
            text,
            images: vec![],
        }
    }
}

impl From<String> for ToolOutput {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}
