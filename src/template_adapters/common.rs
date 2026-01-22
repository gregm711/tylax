use std::collections::HashMap;

use typst_syntax::{SyntaxKind, SyntaxNode};

pub fn find_show_rule_with_prefix(root: &SyntaxNode, prefix: &str) -> Option<(SyntaxNode, String)> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::ShowRule {
            if let Some(func) = node.children().find(|c| c.kind() == SyntaxKind::FuncCall) {
                if let Some(name) = func_call_name(&func) {
                    if name.starts_with(prefix) && name.ends_with(".with") {
                        return Some((node, name));
                    }
                }
            }
        }
        for child in node.children() {
            stack.push(child.clone());
        }
    }
    None
}

pub fn collect_let_bindings(root: &SyntaxNode) -> HashMap<String, SyntaxNode> {
    let mut map = HashMap::new();
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::LetBinding {
            if let Some((name, value)) = parse_let_value(&node) {
                map.insert(name, value);
            }
        }
        for child in node.children() {
            stack.push(child.clone());
        }
    }
    map
}

fn parse_let_value(node: &SyntaxNode) -> Option<(String, SyntaxNode)> {
    let mut name: Option<String> = None;
    let mut value: Option<SyntaxNode> = None;
    let mut seen_eq = false;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Ident if name.is_none() => name = Some(child.text().to_string()),
            SyntaxKind::Eq => seen_eq = true,
            SyntaxKind::Space => {}
            _ => {
                if seen_eq {
                    value = Some(child.clone());
                    break;
                }
            }
        }
    }
    Some((name?, value?))
}

pub fn resolve_ident(node: &SyntaxNode, lets: &HashMap<String, SyntaxNode>) -> SyntaxNode {
    let mut current = node.clone();
    for _ in 0..8 {
        if current.kind() != SyntaxKind::Ident {
            break;
        }
        let name = current.text();
        if let Some(next) = lets.get(name.as_str()) {
            current = next.clone();
            continue;
        }
        break;
    }
    current
}

pub fn extract_named_key(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|c| c.kind() == SyntaxKind::Ident)
        .map(|n| n.text().to_string())
}

pub fn extract_named_value_node(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut seen_colon = false;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Colon => seen_colon = true,
            SyntaxKind::Space | SyntaxKind::Comma => {}
            _ if seen_colon => return Some(child.clone()),
            _ => {}
        }
    }
    None
}

pub fn extract_named_args(show_rule: &SyntaxNode) -> HashMap<String, SyntaxNode> {
    let mut out = HashMap::new();
    let Some(func) = show_rule
        .children()
        .find(|c| c.kind() == SyntaxKind::FuncCall)
    else {
        return out;
    };
    let Some(args) = func.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return out;
    };
    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let Some(key) = extract_named_key(&child) else {
            continue;
        };
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        out.insert(key, value);
    }
    out
}

pub fn extract_array_elements(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut out = Vec::new();
    if node.kind() != SyntaxKind::Array {
        return out;
    }
    for child in node.children() {
        match child.kind() {
            SyntaxKind::LeftParen
            | SyntaxKind::RightParen
            | SyntaxKind::Comma
            | SyntaxKind::Space => {
                continue;
            }
            _ => out.push(child.clone()),
        }
    }
    out
}

pub fn extract_array_strings(node: &SyntaxNode, lets: &HashMap<String, SyntaxNode>) -> Vec<String> {
    let mut out = Vec::new();
    if node.kind() != SyntaxKind::Array {
        return out;
    }
    for child in extract_array_elements(node) {
        let resolved = resolve_ident(&child, lets);
        if let Some(value) = extract_string_like(&resolved, lets) {
            out.push(value);
        }
    }
    out
}

pub fn extract_string_like(
    node: &SyntaxNode,
    lets: &HashMap<String, SyntaxNode>,
) -> Option<String> {
    let resolved = resolve_ident(node, lets);
    match resolved.kind() {
        SyntaxKind::Str => Some(resolved.text().trim_matches('"').to_string()),
        SyntaxKind::Text => Some(resolved.text().to_string()),
        SyntaxKind::Ident => Some(resolved.text().to_string()),
        SyntaxKind::ContentBlock | SyntaxKind::Markup => Some(extract_markup_text(&resolved)),
        SyntaxKind::Array => {
            let values = extract_array_strings(&resolved, lets);
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        }
        _ => Some(node_full_text(&resolved)),
    }
}

fn extract_markup_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Text | SyntaxKind::Str => {
                out.push_str(child.text().trim_matches('"'));
            }
            SyntaxKind::Space => out.push(' '),
            _ => out.push_str(&extract_markup_text(&child)),
        }
    }
    out
}

pub fn extract_option_bool(
    node: &SyntaxNode,
    lets: &HashMap<String, SyntaxNode>,
) -> Option<Option<bool>> {
    let resolved = resolve_ident(node, lets);
    let raw = resolved.text().trim().to_lowercase();
    match raw.as_str() {
        "none" => Some(None),
        "true" => Some(Some(true)),
        "false" => Some(Some(false)),
        _ => None,
    }
}

pub fn extract_dict_entries(
    node: &SyntaxNode,
    lets: &HashMap<String, SyntaxNode>,
) -> Vec<(String, SyntaxNode)> {
    let mut out = Vec::new();
    let resolved = resolve_ident(node, lets);
    if resolved.kind() != SyntaxKind::Dict {
        return out;
    }
    for child in resolved.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let Some(key) = extract_named_key(&child) else {
            continue;
        };
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        out.push((key, value));
    }
    out
}

pub fn func_call_name(node: &SyntaxNode) -> Option<String> {
    let first = node.children().next()?;
    if first.kind() == SyntaxKind::Ident {
        return Some(first.text().to_string());
    }
    if first.kind() == SyntaxKind::FieldAccess {
        let mut parts = Vec::new();
        for child in first.children() {
            if child.kind() == SyntaxKind::Ident {
                parts.push(child.text().to_string());
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("."));
        }
    }
    None
}

pub fn node_full_text(node: &SyntaxNode) -> String {
    node.clone().into_text().to_string()
}

pub fn extract_year_from_name(name: &str, prefix: &str) -> Option<String> {
    let trimmed = name.strip_prefix(prefix)?;
    let trimmed = trimmed.trim_start_matches(|ch: char| !ch.is_ascii_digit());
    if trimmed.is_empty() {
        return None;
    }
    let digits: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

pub fn escape_latex(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '^' => out.push_str("\\^{}"),
            '~' => out.push_str("\\~{}"),
            _ => out.push(ch),
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Affiliation {
    pub lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SimpleAuthor {
    pub name: String,
    pub affls: Vec<String>,
    pub email: Option<String>,
    pub equal: bool,
}

pub fn render_authors_simple(
    authors: &[SimpleAuthor],
    affls: &HashMap<String, Affiliation>,
    separator: &str,
) -> String {
    let mut out = String::new();
    out.push_str("\\author{\n");
    for (idx, author) in authors.iter().enumerate() {
        if idx > 0 {
            out.push_str(separator);
            out.push('\n');
        }
        out.push_str(&escape_latex(&author.name));
        let mut lines: Vec<(String, bool)> = Vec::new();
        for key in &author.affls {
            if let Some(affl) = affls.get(key) {
                for line in &affl.lines {
                    lines.push((line.clone(), false));
                }
            } else {
                lines.push((key.clone(), false));
            }
        }
        if let Some(email) = &author.email {
            lines.push((format!("\\texttt{{{}}}", escape_latex(email)), true));
        }
        for (line, raw) in lines {
            out.push_str(" \\\\\n");
            if raw {
                out.push_str(&line);
            } else {
                out.push_str(&escape_latex(&line));
            }
        }
        out.push('\n');
    }
    out.push_str("}\n");
    out
}

pub fn parse_authors_with_affls(
    node: &SyntaxNode,
    lets: &HashMap<String, SyntaxNode>,
) -> (Vec<SimpleAuthor>, HashMap<String, Affiliation>) {
    let resolved = resolve_ident(node, lets);
    if resolved.kind() == SyntaxKind::Array {
        let elems = extract_array_elements(&resolved);
        if elems.len() == 2 {
            let authors_node = resolve_ident(&elems[0], lets);
            let affls_node = resolve_ident(&elems[1], lets);
            let authors = parse_author_list(&authors_node, lets);
            let affls = parse_affl_map(&affls_node, lets);
            return (authors, affls);
        }
    }
    (parse_author_list(&resolved, lets), HashMap::new())
}

fn parse_author_list(node: &SyntaxNode, lets: &HashMap<String, SyntaxNode>) -> Vec<SimpleAuthor> {
    let mut authors = Vec::new();
    let resolved = resolve_ident(node, lets);
    if resolved.kind() != SyntaxKind::Array {
        return authors;
    }
    for child in extract_array_elements(&resolved) {
        let resolved = resolve_ident(&child, lets);
        if resolved.kind() == SyntaxKind::Dict {
            if let Some(author) = parse_author_dict(&resolved, lets) {
                authors.push(author);
            }
        }
    }
    authors
}

fn parse_author_dict(
    node: &SyntaxNode,
    lets: &HashMap<String, SyntaxNode>,
) -> Option<SimpleAuthor> {
    let mut author = SimpleAuthor {
        name: String::new(),
        affls: Vec::new(),
        email: None,
        equal: false,
    };
    for child in node.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child)?;
        let value = extract_named_value_node(&child)?;
        match key.as_str() {
            "name" => {
                if let Some(name) = extract_string_like(&value, lets) {
                    author.name = name;
                }
            }
            "affl" | "affls" | "affiliation" | "affiliations" => {
                author.affls = parse_affl_keys(&value, lets);
            }
            "email" => {
                author.email = extract_string_like(&value, lets);
            }
            "equal" => {
                if let Some(Some(val)) = extract_option_bool(&value, lets) {
                    author.equal = val;
                }
            }
            _ => {}
        }
    }
    if author.name.is_empty() {
        None
    } else {
        Some(author)
    }
}

fn parse_affl_keys(node: &SyntaxNode, lets: &HashMap<String, SyntaxNode>) -> Vec<String> {
    let resolved = resolve_ident(node, lets);
    if resolved.kind() == SyntaxKind::Array {
        return extract_array_strings(&resolved, lets);
    }
    if let Some(value) = extract_string_like(&resolved, lets) {
        let trimmed = value.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            let inner = &trimmed[1..trimmed.len() - 1];
            let mut keys = Vec::new();
            for part in inner.split(',') {
                let token = part.trim().trim_matches('"').trim_matches('\'');
                if !token.is_empty() {
                    keys.push(token.to_string());
                }
            }
            if !keys.is_empty() {
                return keys;
            }
        }
        return vec![value];
    }
    Vec::new()
}

fn parse_affl_map(
    node: &SyntaxNode,
    lets: &HashMap<String, SyntaxNode>,
) -> HashMap<String, Affiliation> {
    let mut out = HashMap::new();
    let resolved = resolve_ident(node, lets);
    if resolved.kind() != SyntaxKind::Dict {
        return out;
    }
    for (key, value) in extract_dict_entries(&resolved, lets) {
        let lines = parse_affl_lines(&value, lets);
        if !lines.is_empty() {
            out.insert(key, Affiliation { lines });
        }
    }
    out
}

fn parse_affl_lines(node: &SyntaxNode, lets: &HashMap<String, SyntaxNode>) -> Vec<String> {
    let resolved = resolve_ident(node, lets);
    match resolved.kind() {
        SyntaxKind::Array => extract_array_strings(&resolved, lets),
        SyntaxKind::Dict => {
            let mut lines = Vec::new();
            let entries = extract_dict_entries(&resolved, lets);
            let preferred = ["department", "institution", "location", "country"];
            let mut used: Vec<String> = Vec::new();
            for key in preferred.iter() {
                if let Some((_, value)) = entries.iter().find(|(k, _)| k == key) {
                    if let Some(value) = extract_string_like(value, lets) {
                        lines.push(value);
                        used.push((*key).to_string());
                    }
                }
            }
            for (key, value) in entries {
                if used.contains(&key) {
                    continue;
                }
                if let Some(value) = extract_string_like(&value, lets) {
                    lines.push(value);
                }
            }
            lines
        }
        _ => extract_string_like(&resolved, lets)
            .map(|v| vec![v])
            .unwrap_or_default(),
    }
}
