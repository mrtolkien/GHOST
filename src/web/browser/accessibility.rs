use std::collections::HashMap;
use std::fmt::Write;

use chromiumoxide::cdp::browser_protocol::accessibility::{AxNode as CdpAxNode, AxPropertyName};

/// A node in the parsed accessibility tree.
#[derive(Debug, Clone)]
pub struct AxNode {
    pub role: String,
    pub name: String,
    pub backend_node_id: Option<i64>,
    pub properties: AxProperties,
    pub children: Vec<AxNode>,
}

#[derive(Debug, Clone, Default)]
pub struct AxProperties {
    pub level: Option<u32>,
    pub value: Option<String>,
    pub checked: Option<bool>,
    pub expanded: Option<bool>,
}

/// Parse Chrome's flat AXNode array into a tree of our `AxNode`.
///
/// Chrome returns nodes in a flat array with `child_ids` references.
/// We build a lookup from `AxNodeId` to index, then reconstruct the tree
/// recursively. Ignored nodes are skipped. The root node itself (usually
/// `RootWebArea`) is omitted — its children become the returned top-level
/// nodes.
pub fn parse_ax_tree(raw_nodes: &[CdpAxNode]) -> Vec<AxNode> {
    // Map AxNodeId → index in the flat array.
    let id_to_idx: HashMap<&str, usize> = raw_nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.node_id.as_ref(), i))
        .collect();

    // Find the root: first non-ignored node.
    let root_idx = match raw_nodes.iter().position(|n| !n.ignored) {
        Some(idx) => idx,
        None => return Vec::new(),
    };

    // Build the tree starting from the root, then return its children
    // (skipping the RootWebArea wrapper).
    let root = build_node(raw_nodes, root_idx, &id_to_idx);
    match root {
        Some(n) => n.children,
        None => Vec::new(),
    }
}

/// Recursively build an `AxNode` from the flat array.
fn build_node(raw: &[CdpAxNode], idx: usize, id_map: &HashMap<&str, usize>) -> Option<AxNode> {
    let cdp = &raw[idx];

    if cdp.ignored {
        return None;
    }

    let role = extract_ax_str(&cdp.role);
    let name = extract_ax_str(&cdp.name);
    let backend_node_id = cdp.backend_dom_node_id.as_ref().map(|id| *id.inner());
    let properties = extract_properties(cdp);

    let children: Vec<AxNode> = cdp
        .child_ids
        .as_ref()
        .map(|ids| {
            ids.iter()
                .filter_map(|child_id| {
                    let child_idx = id_map.get(child_id.as_ref())?;
                    build_node(raw, *child_idx, id_map)
                })
                .collect()
        })
        .unwrap_or_default();

    Some(AxNode {
        role,
        name,
        backend_node_id,
        properties,
        children,
    })
}

/// Extract a string value from an optional `AXValue`.
fn extract_ax_str(
    ax_val: &Option<chromiumoxide::cdp::browser_protocol::accessibility::AxValue>,
) -> String {
    ax_val
        .as_ref()
        .and_then(|v| v.value.as_ref())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Extract our `AxProperties` from a CDP node's properties list.
fn extract_properties(cdp: &CdpAxNode) -> AxProperties {
    let mut props = AxProperties::default();

    // The CDP node also has a top-level `value` field.
    if let Some(ref ax_val) = cdp.value
        && let Some(ref v) = ax_val.value
    {
        props.value = v.as_str().map(String::from);
    }

    let Some(ref prop_list) = cdp.properties else {
        return props;
    };

    for prop in prop_list {
        match prop.name {
            AxPropertyName::Level => {
                if let Some(ref v) = prop.value.value {
                    props.level = v.as_u64().map(|n| n as u32);
                }
            }
            AxPropertyName::Checked => {
                if let Some(ref v) = prop.value.value {
                    // CDP sends checked as "true"/"false" strings or booleans.
                    props.checked = match v {
                        serde_json::Value::Bool(b) => Some(*b),
                        serde_json::Value::String(s) => Some(s == "true"),
                        _ => None,
                    };
                }
            }
            AxPropertyName::Expanded => {
                if let Some(ref v) = prop.value.value {
                    props.expanded = match v {
                        serde_json::Value::Bool(b) => Some(*b),
                        serde_json::Value::String(s) => Some(s == "true"),
                        _ => None,
                    };
                }
            }
            _ => {}
        }
    }

    props
}

/// Classification that determines whether a node gets a ref ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleClass {
    Interactive,
    Content,
    Structural,
}

pub fn classify_role(role: &str) -> RoleClass {
    match role {
        "button" | "checkbox" | "combobox" | "link" | "listbox" | "menuitem"
        | "menuitemcheckbox" | "menuitemradio" | "option" | "radio" | "searchbox" | "slider"
        | "spinbutton" | "switch" | "tab" | "textbox" | "treeitem" => RoleClass::Interactive,
        "cell" | "columnheader" | "heading" | "img" | "listitem" | "rowheader" => {
            RoleClass::Content
        }
        _ => RoleClass::Structural,
    }
}

/// Maps ref IDs ("e1", "e2", ...) to Chrome BackendDOMNodeIds.
///
/// Refs are sequential, assigned in depth-first tree order during snapshot.
/// The map is invalidated on every `snapshot()` call — `reset()` clears all
/// refs and restarts the counter from 1.
#[derive(Debug)]
pub struct RefMap {
    refs: HashMap<String, i64>,
    counter: u32,
}

impl RefMap {
    pub fn new() -> Self {
        Self {
            refs: HashMap::new(),
            counter: 0,
        }
    }

    pub fn assign(&mut self, backend_node_id: i64) -> String {
        self.counter += 1;
        let ref_id = format!("e{}", self.counter);
        self.refs.insert(ref_id.clone(), backend_node_id);
        ref_id
    }

    pub fn resolve(&self, ref_id: &str) -> Option<i64> {
        self.refs.get(ref_id).copied()
    }

    pub fn reset(&mut self) {
        self.refs.clear();
        self.counter = 0;
    }
}

impl Default for RefMap {
    fn default() -> Self {
        Self::new()
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Render an accessibility tree as compact XML.
///
/// - `roots`: top-level tree nodes
/// - `refs`: mutable ref map — refs are assigned during traversal
/// - `max_nodes`: max number of nodes to render (all nodes count)
/// - `max_depth`: max nesting depth (0-indexed from roots)
/// - `offset`: skip first N nodes in depth-first order (refs still assigned)
pub fn render_xml(
    roots: &[AxNode],
    refs: &mut RefMap,
    max_nodes: usize,
    max_depth: usize,
    offset: usize,
) -> String {
    let total = count_nodes(roots);
    let mut buf = String::new();
    let mut counter: usize = 0;
    let mut rendered: usize = 0;
    let mut truncated = false;

    for node in roots {
        render_node(
            node,
            refs,
            max_nodes,
            max_depth,
            offset,
            0,
            &mut buf,
            &mut counter,
            &mut rendered,
            &mut truncated,
            total,
        );
    }

    buf
}

fn count_nodes(nodes: &[AxNode]) -> usize {
    let mut total = 0;
    for node in nodes {
        total += 1;
        total += count_nodes(&node.children);
    }
    total
}

/// Returns true if this node should be assigned a ref.
fn should_assign_ref(node: &AxNode) -> bool {
    if node.role == "StaticText" {
        return false;
    }
    if node.backend_node_id.is_none() {
        return false;
    }
    match classify_role(&node.role) {
        RoleClass::Interactive => true,
        RoleClass::Content => !node.name.is_empty(),
        RoleClass::Structural => false,
    }
}

/// Returns true if this leaf node should be omitted (empty nameless leaf).
fn is_empty_nameless_leaf(node: &AxNode) -> bool {
    node.children.is_empty() && node.name.is_empty() && node.role != "StaticText"
}

#[allow(clippy::too_many_arguments)]
fn render_node(
    node: &AxNode,
    refs: &mut RefMap,
    max_nodes: usize,
    max_depth: usize,
    offset: usize,
    depth: usize,
    buf: &mut String,
    counter: &mut usize,
    rendered: &mut usize,
    truncated: &mut bool,
    total: usize,
) {
    if *truncated {
        // Still need to assign refs for consistency, even after truncation.
        assign_refs_only(node, refs);
        return;
    }

    *counter += 1;

    // Assign ref if needed (always, even for skipped/offset nodes).
    let ref_id = if should_assign_ref(node) {
        Some(refs.assign(node.backend_node_id.unwrap()))
    } else {
        None
    };

    let in_offset = *counter <= offset;

    // Check truncation limit.
    if !in_offset {
        if *rendered >= max_nodes {
            *truncated = true;
            let _ = writeln!(
                buf,
                "<!-- Snapshot truncated: showing {max_nodes} of {total} nodes. \
                 Use offset={} to see more. -->",
                offset + max_nodes,
            );
            // Assign refs for remaining children.
            for child in &node.children {
                assign_refs_only(child, refs);
            }
            return;
        }
        *rendered += 1;
    }

    // Skip rendering if within offset or an empty nameless leaf.
    if in_offset {
        // Still recurse children for counting and ref assignment.
        for child in &node.children {
            render_node(
                child,
                refs,
                max_nodes,
                max_depth,
                offset,
                depth + 1,
                buf,
                counter,
                rendered,
                truncated,
                total,
            );
        }
        return;
    }

    // Omit empty nameless leaves.
    if is_empty_nameless_leaf(node) {
        return;
    }

    // Depth limit.
    if depth > max_depth {
        let indent = "  ".repeat(depth);
        let _ = writeln!(buf, "{indent}<!-- ... -->");
        // Still recurse for ref assignment.
        for child in &node.children {
            assign_refs_only(child, refs);
        }
        return;
    }

    let indent = "  ".repeat(depth);
    let tag = if node.role == "StaticText" {
        "text"
    } else {
        &node.role
    };

    // Build attributes.
    let mut attrs = String::new();

    // Level (headings).
    if let Some(level) = node.properties.level {
        let _ = write!(attrs, r#" level="{level}""#);
    }

    // Ref attribute.
    if let Some(ref r) = ref_id {
        let _ = write!(attrs, r#" ref="{r}""#);
    }

    // Checked.
    if let Some(checked) = node.properties.checked {
        let _ = write!(attrs, r#" checked="{checked}""#);
    }

    // Expanded.
    if let Some(expanded) = node.properties.expanded {
        let _ = write!(attrs, r#" expanded="{expanded}""#);
    }

    // For interactive nodes with a value, render name as attribute.
    let has_value = node.properties.value.is_some();
    let render_name_as_attr = has_value && !node.name.is_empty();

    if render_name_as_attr {
        let _ = write!(attrs, r#" name="{}""#, xml_escape(&node.name));
    }

    // Value attribute.
    if let Some(ref val) = node.properties.value {
        let _ = write!(attrs, r#" value="{}""#, xml_escape(val));
    }

    // Structural nodes with a name: render name as attribute.
    // StaticText is excluded — it always renders name as text content.
    let is_static_text = node.role == "StaticText";
    let role_class = classify_role(&node.role);
    let structural_with_name = role_class == RoleClass::Structural
        && !node.name.is_empty()
        && !render_name_as_attr
        && !is_static_text;
    if structural_with_name {
        let _ = write!(attrs, r#" name="{}""#, xml_escape(&node.name));
    }

    let escaped_name = xml_escape(&node.name);

    if !node.children.is_empty() {
        // Node with children: open tag, children, close tag.
        let _ = writeln!(buf, "{indent}<{tag}{attrs}>");
        for child in &node.children {
            render_node(
                child,
                refs,
                max_nodes,
                max_depth,
                offset,
                depth + 1,
                buf,
                counter,
                rendered,
                truncated,
                total,
            );
        }
        if !*truncated {
            let _ = writeln!(buf, "{indent}</{tag}>");
        } else {
            // If truncation happened inside children, still close the tag.
            let _ = writeln!(buf, "{indent}</{tag}>");
        }
    } else if render_name_as_attr || (structural_with_name && node.name.is_empty()) {
        // Self-closing with attributes.
        let _ = writeln!(buf, "{indent}<{tag}{attrs} />");
    } else if !node.name.is_empty() && !structural_with_name && !render_name_as_attr {
        // Leaf with name as text content.
        let _ = writeln!(buf, "{indent}<{tag}{attrs}>{escaped_name}</{tag}>");
    } else if !attrs.is_empty() {
        // Has attributes but no text name to show as content — self-closing.
        let _ = writeln!(buf, "{indent}<{tag}{attrs} />");
    } else {
        // Empty, already handled by is_empty_nameless_leaf check above for most
        // cases, but structural nodes with name="" could still reach here.
        // Omit entirely.
    }
}

/// Recursively assign refs without rendering (for skipped/truncated subtrees).
fn assign_refs_only(node: &AxNode, refs: &mut RefMap) {
    if should_assign_ref(node) {
        refs.assign(node.backend_node_id.unwrap());
    }
    for child in &node.children {
        assign_refs_only(child, refs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_roles_get_refs() {
        assert_eq!(classify_role("button"), RoleClass::Interactive);
        assert_eq!(classify_role("textbox"), RoleClass::Interactive);
        assert_eq!(classify_role("link"), RoleClass::Interactive);
    }

    #[test]
    fn content_roles_get_refs_when_named() {
        assert_eq!(classify_role("heading"), RoleClass::Content);
        assert_eq!(classify_role("img"), RoleClass::Content);
    }

    #[test]
    fn structural_roles_never_get_refs() {
        assert_eq!(classify_role("list"), RoleClass::Structural);
        assert_eq!(classify_role("navigation"), RoleClass::Structural);
        assert_eq!(classify_role("main"), RoleClass::Structural);
    }

    #[test]
    fn unknown_roles_treated_as_structural() {
        assert_eq!(classify_role("banana"), RoleClass::Structural);
    }

    #[test]
    fn ref_map_assigns_sequential_ids() {
        let mut refs = RefMap::new();
        assert_eq!(refs.assign(100), "e1");
        assert_eq!(refs.assign(200), "e2");
    }

    #[test]
    fn ref_map_resolves_ids() {
        let mut refs = RefMap::new();
        refs.assign(42);
        assert_eq!(refs.resolve("e1"), Some(42));
        assert_eq!(refs.resolve("e99"), None);
    }

    #[test]
    fn ref_map_reset_clears_and_restarts() {
        let mut refs = RefMap::new();
        refs.assign(1);
        refs.reset();
        let r = refs.assign(2);
        assert_eq!(r, "e1");
        assert_eq!(refs.resolve("e1"), Some(2));
    }

    #[test]
    fn render_simple_tree() {
        let tree = vec![
            AxNode {
                role: "heading".into(),
                name: "Hello".into(),
                backend_node_id: Some(1),
                properties: AxProperties {
                    level: Some(1),
                    ..Default::default()
                },
                children: vec![],
            },
            AxNode {
                role: "button".into(),
                name: "Click me".into(),
                backend_node_id: Some(2),
                properties: AxProperties::default(),
                children: vec![],
            },
        ];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains(r#"<heading level="1" ref="e1">Hello</heading>"#));
        assert!(xml.contains(r#"<button ref="e2">Click me</button>"#));
    }

    #[test]
    fn render_nested_structure() {
        let tree = vec![AxNode {
            role: "navigation".into(),
            name: "Main".into(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![AxNode {
                role: "link".into(),
                name: "Home".into(),
                backend_node_id: Some(10),
                properties: AxProperties::default(),
                children: vec![],
            }],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains(r#"<navigation name="Main">"#));
        assert!(xml.contains(r#"  <link ref="e1">Home</link>"#));
        assert!(xml.contains("</navigation>"));
    }

    #[test]
    fn structural_nodes_get_no_ref() {
        let tree = vec![AxNode {
            role: "list".into(),
            name: String::new(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(!xml.contains("ref="));
    }

    #[test]
    fn text_nodes_render_without_ref() {
        let tree = vec![AxNode {
            role: "StaticText".into(),
            name: "Hello world".into(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains("<text>Hello world</text>"));
        assert!(!xml.contains("ref="));
    }

    #[test]
    fn xml_escapes_special_chars() {
        let tree = vec![AxNode {
            role: "button".into(),
            name: "A < B & C".into(),
            backend_node_id: Some(1),
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains("A &lt; B &amp; C"));
    }

    #[test]
    fn truncation_at_node_limit() {
        let tree: Vec<AxNode> = (0..10)
            .map(|i| AxNode {
                role: "button".into(),
                name: format!("Button {i}"),
                backend_node_id: Some(i as i64),
                properties: AxProperties::default(),
                children: vec![],
            })
            .collect();
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 5, 15, 0);
        assert!(xml.contains("Button 0"));
        assert!(xml.contains("Button 4"));
        assert!(!xml.contains("Button 5"));
        assert!(xml.contains("<!-- Snapshot truncated:"));
    }

    #[test]
    fn empty_nameless_leaf_omitted() {
        let tree = vec![AxNode {
            role: "group".into(),
            name: String::new(),
            backend_node_id: None,
            properties: AxProperties::default(),
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.is_empty() || xml.trim().is_empty());
    }

    #[test]
    fn content_node_with_name_gets_ref() {
        let tree = vec![AxNode {
            role: "heading".into(),
            name: "Title".into(),
            backend_node_id: Some(1),
            properties: AxProperties {
                level: Some(2),
                ..Default::default()
            },
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains(r#"ref="e1""#));
    }

    #[test]
    fn content_node_without_name_gets_no_ref() {
        let tree = vec![AxNode {
            role: "heading".into(),
            name: String::new(),
            backend_node_id: Some(1),
            properties: AxProperties {
                level: Some(2),
                ..Default::default()
            },
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(!xml.contains("ref="));
    }

    #[test]
    fn value_attribute_on_textbox() {
        let tree = vec![AxNode {
            role: "textbox".into(),
            name: "Email".into(),
            backend_node_id: Some(1),
            properties: AxProperties {
                value: Some("john@test.com".into()),
                ..Default::default()
            },
            children: vec![],
        }];
        let mut refs = RefMap::new();
        let xml = render_xml(&tree, &mut refs, 500, 15, 0);
        assert!(xml.contains(r#"name="Email""#));
        assert!(xml.contains(r#"value="john@test.com""#));
        assert!(xml.contains(r#"ref="e1""#));
    }
}
