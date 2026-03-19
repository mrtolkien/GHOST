use std::collections::HashMap;

/// Render a template string by replacing `{{ variable }}` placeholders with
/// values from the provided map. Unknown variables render as empty strings
/// and emit a warning.
#[tracing::instrument(skip_all, level = "debug")]
pub fn render_template(template: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        let (prefix, after_open) = rest.split_at(start);
        out.push_str(prefix);

        let Some(end) = after_open.find("}}") else {
            // Unclosed {{ — emit as-is and stop scanning.
            out.push_str(after_open);
            return out;
        };

        let var_name = after_open[2..end].trim();

        if var_name.is_empty() {
            // Empty `{{}}` — skip it silently.
            rest = &after_open[end + 2..];
            continue;
        }

        match vars.get(var_name) {
            Some(value) => out.push_str(value),
            None => {
                tracing::warn!("Unknown template variable '{var_name}'");
            }
        }

        rest = &after_open[end + 2..];
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_variable_renders_empty() {
        let vars = HashMap::new();
        let result = render_template("before {{ unknown }} after", &vars);
        assert_eq!(result, "before  after");
    }

    #[test]
    fn unclosed_brace_emitted_as_is() {
        let vars = HashMap::new();
        let result = render_template("before {{ oops", &vars);
        assert_eq!(result, "before {{ oops");
    }
}
