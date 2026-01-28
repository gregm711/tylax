//! Utility functions for LaTeX to Typst conversion
//!
//! This module contains pure utility functions that don't depend on converter state.

use crate::data::symbols::GREEK_LETTERS;
use mitex_parser::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

// =============================================================================
// Text Processing Utilities
// =============================================================================

/// Sanitize a label name for Typst compatibility
/// Converts colons to hyphens since Typst labels work better with hyphens
pub fn sanitize_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut prev_dash = false;
    for ch in label.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(mapped);
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        label.trim().to_string()
    } else {
        trimmed
    }
}

/// Sanitize citation keys for Typst compatibility (allow only alphanumeric and hyphen).
pub fn sanitize_citation_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut prev_dash = false;
    for ch in key.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || ch == '-' {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(mapped);
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        key.trim().to_string()
    } else {
        trimmed
    }
}

/// Collect bibliography entries from LaTeX source.
pub fn collect_bibliography_entries(input: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && (input[i..].starts_with("\\bibliography")
                || input[i..].starts_with("\\addbibresource")
                || input[i..].starts_with("\\bibdata"))
        {
            let (cmd, after) = if input[i..].starts_with("\\bibliography") {
                ("\\bibliography", i + "\\bibliography".len())
            } else if input[i..].starts_with("\\addbibresource") {
                ("\\addbibresource", i + "\\addbibresource".len())
            } else {
                ("\\bibdata", i + "\\bibdata".len())
            };
            // Avoid \bibliographystyle
            if cmd == "\\bibliography" && after < bytes.len() && bytes[after].is_ascii_alphabetic()
            {
                i += 1;
                continue;
            }
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let (content, used) = extract_braced_content(&input[j..]);
                if let Some(raw) = content {
                    for part in raw.split(',') {
                        let trimmed = part.trim();
                        if !trimmed.is_empty() {
                            entries.push(trimmed.to_string());
                        }
                    }
                    i = j + used;
                    continue;
                }
            }
        }
        i += 1;
    }
    entries
}

/// Detect a thebibliography environment in the source.
pub fn contains_thebibliography_env(input: &str) -> bool {
    input.contains("\\begin{thebibliography}")
}

/// Collect \graphicspath{{...}{...}} entries from LaTeX source.
pub fn collect_graphicspath_entries(input: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && input[i..].starts_with("\\graphicspath") {
            let after = i + "\\graphicspath".len();
            if after < bytes.len() && bytes[after].is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let (content, used) = extract_braced_content(&input[j..]);
                if let Some(raw) = content {
                    let mut groups = parse_braced_groups(&raw);
                    if groups.is_empty() {
                        let trimmed = raw.trim();
                        if !trimmed.is_empty() {
                            groups.push(trimmed.to_string());
                        }
                    }
                    for group in groups {
                        let trimmed = group.trim();
                        if !trimmed.is_empty() {
                            entries.push(trimmed.to_string());
                        }
                    }
                    i = j + used;
                    continue;
                }
            }
        }
        i += 1;
    }
    entries
}

/// Collect \includegraphics{...} paths from LaTeX source.
pub fn collect_includegraphics_paths(input: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && input[i..].starts_with("\\includegraphics") {
            let mut j = i + "\\includegraphics".len();
            if j < bytes.len() && bytes[j] == b'*' {
                j += 1;
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'[' {
                let mut depth = 0i32;
                while j < bytes.len() {
                    match bytes[j] {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
            }
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let (content, used) = extract_braced_content(&input[j..]);
                if let Some(path) = content {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        entries.push(trimmed.to_string());
                    }
                    i = j + used;
                    continue;
                }
            }
        }
        i += 1;
    }
    entries
}

/// Sanitize BibTeX content for Typst compatibility.
pub fn sanitize_bibtex_content(input: &str) -> String {
    let converted = convert_string_entries(input);
    let mut sanitized = sanitize_bibtex_keys(&converted);
    // Normalize empty year fields to a sentinel value to keep Typst's BibLaTeX parser happy.
    sanitized = sanitized.replace("year = {},", "year = {0000},");
    sanitized = sanitized.replace("year = {}", "year = {0000}");
    sanitized = sanitized.replace("year = \"\",", "year = {0000},");
    sanitized = sanitized.replace("year = \"\"", "year = {0000}");
    let (mut sanitized, string_map) = normalize_bibtex_field_values(&sanitized);
    sanitized = expand_bare_bibtex_values(&sanitized, &string_map);
    sanitized
}

/// Normalize BibTeX field values to avoid unresolved abbreviations and concatenations.
fn normalize_bibtex_field_values(input: &str) -> (String, HashMap<String, String>) {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut string_map: HashMap<String, String> = HashMap::new();

    while i < bytes.len() {
        if bytes[i] == b'@' {
            if let Some((entry, next)) = normalize_bibtex_entry(input, i, &mut string_map) {
                out.extend_from_slice(entry.as_bytes());
                i = next;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    (
        String::from_utf8(out).unwrap_or_else(|_| input.to_string()),
        string_map,
    )
}

fn expand_bare_bibtex_values(input: &str, string_map: &HashMap<String, String>) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut in_quote = false;

    while i < bytes.len() {
        let b = bytes[i];
        let ch = b as char;
        if ch == '"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_quote = !in_quote;
            out.push(b);
            i += 1;
            continue;
        }
        if !in_quote {
            if ch == '{' {
                depth += 1;
                out.push(b'{');
                i += 1;
                continue;
            }
            if ch == '}' {
                if depth > 0 {
                    depth -= 1;
                }
                out.push(b'}');
                i += 1;
                continue;
            }
        }

        if ch == '=' && depth == 1 && !in_quote {
            out.push(b'=');
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                out.push(bytes[i]);
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let next = bytes[i] as char;
            if next == '{' || next == '"' {
                continue;
            }
            let (value, next_idx) = parse_bibtex_value(input, i);
            let normalized = normalize_bibtex_value(&value, string_map);
            out.push(b'{');
            out.extend_from_slice(normalized.as_bytes());
            out.push(b'}');
            if next_idx > 0 && input.as_bytes()[next_idx - 1] == b',' {
                out.push(b',');
            }
            i = next_idx;
            continue;
        }

        out.push(b);
        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn normalize_bibtex_entry(
    input: &str,
    start: usize,
    string_map: &mut HashMap<String, String>,
) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = start + 1;
    if i >= len || !bytes[i].is_ascii_alphabetic() {
        return None;
    }

    let mut entry_type = String::new();
    while i < len && bytes[i].is_ascii_alphabetic() {
        entry_type.push(bytes[i] as char);
        i += 1;
    }
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= len || (bytes[i] != b'{' && bytes[i] != b'(') {
        return None;
    }
    let open = bytes[i] as char;
    let close = if open == '{' { '}' } else { ')' };
    i += 1;
    let body_start = i;

    let mut depth = 1i32;
    let mut in_quote = false;
    while i < len && depth > 0 {
        let ch = bytes[i] as char;
        if ch == '"' && depth == 1 && (i == 0 || bytes[i - 1] != b'\\') {
            in_quote = !in_quote;
        }
        if !in_quote {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
            }
        }
        i += 1;
    }
    if depth != 0 {
        return None;
    }
    let body_end = i - 1;
    let body = &input[body_start..body_end];

    let entry_type_lower = entry_type.to_lowercase();
    let mut entry_text = if entry_type_lower == "string" {
        normalize_bibtex_string_entry(&entry_type, body, open, close, string_map);
        String::new()
    } else if entry_type_lower == "preamble" || entry_type_lower == "comment" {
        format!("@{}{}{}{}", entry_type, open, body, close)
    } else {
        normalize_bibtex_regular_entry(&entry_type, body, open, close, string_map)
    };
    if !entry_text.is_empty() && !entry_text.ends_with('\n') {
        entry_text.push('\n');
    }

    Some((entry_text, i))
}

fn normalize_bibtex_string_entry(
    entry_type: &str,
    body: &str,
    open: char,
    close: char,
    string_map: &mut HashMap<String, String>,
) -> String {
    let bytes = body.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let key_start = idx;
    while idx < bytes.len()
        && (bytes[idx].is_ascii_alphanumeric() || bytes[idx] == b'-' || bytes[idx] == b'_')
    {
        idx += 1;
    }
    let key = body[key_start..idx].trim().to_string();
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx >= bytes.len() || bytes[idx] != b'=' {
        return format!("@{}{}{}{}", entry_type, open, body, close);
    }
    idx += 1;
    let (value, _) = parse_bibtex_value(body, idx);
    let normalized = normalize_bibtex_value(&value, string_map);
    if !key.is_empty() && !normalized.is_empty() {
        string_map.insert(key.to_lowercase(), normalized.clone());
    }
    format!(
        "@{}{}{} = {{{}}}{}",
        entry_type, open, key, normalized, close
    )
}

fn normalize_bibtex_regular_entry(
    entry_type: &str,
    body: &str,
    open: char,
    close: char,
    string_map: &HashMap<String, String>,
) -> String {
    let bytes = body.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let key_start = idx;
    while idx < bytes.len() && bytes[idx] != b',' {
        idx += 1;
    }
    let key = body[key_start..idx].trim().to_string();
    if idx < bytes.len() && bytes[idx] == b',' {
        idx += 1;
    }

    let mut fields: Vec<(String, String)> = Vec::new();
    while idx < bytes.len() {
        while idx < bytes.len() && (bytes[idx].is_ascii_whitespace() || bytes[idx] == b',') {
            idx += 1;
        }
        if idx >= bytes.len() {
            break;
        }
        let field_start = idx;
        while idx < bytes.len()
            && (bytes[idx].is_ascii_alphanumeric() || matches!(bytes[idx], b'_' | b'-' | b':'))
        {
            idx += 1;
        }
        let field = body[field_start..idx].trim().to_string();
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= bytes.len() || bytes[idx] != b'=' {
            break;
        }
        idx += 1;
        let (value, next) = parse_bibtex_value(body, idx);
        idx = next;
        if !field.is_empty() {
            let normalized = normalize_bibtex_value(&value, string_map);
            fields.push((field, normalized));
        }
    }

    // Entry types that require a year field for Typst's BibLaTeX parser
    let needs_year = matches!(
        entry_type.to_lowercase().as_str(),
        "article"
            | "book"
            | "booklet"
            | "inbook"
            | "incollection"
            | "inproceedings"
            | "conference"
            | "manual"
            | "mastersthesis"
            | "phdthesis"
            | "proceedings"
            | "techreport"
            | "unpublished"
            | "misc"
    );

    // Check if year field exists
    let has_year = fields
        .iter()
        .any(|(f, _)| f.eq_ignore_ascii_case("year"));

    // Inject a placeholder year if missing and required
    if needs_year && !has_year {
        fields.push(("year".to_string(), "0000".to_string()));
    }

    // Filter out empty fields that cause Typst's BibLaTeX parser to fail
    let fields: Vec<_> = fields
        .into_iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .collect();

    let mut out = String::new();
    out.push('@');
    out.push_str(entry_type);
    out.push(open);
    out.push_str(&key);
    if !fields.is_empty() {
        out.push(',');
        out.push('\n');
        for (field, value) in fields {
            out.push_str("  ");
            out.push_str(&field);
            out.push_str(" = {");
            out.push_str(&value);
            out.push_str("},\n");
        }
    }
    out.push(close);
    out
}

fn parse_bibtex_value(body: &str, start: usize) -> (String, usize) {
    let bytes = body.as_bytes();
    let mut idx = start;
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    let mut depth = 0i32;
    let mut in_quote = false;
    let value_start = idx;
    while idx < bytes.len() {
        let ch = bytes[idx] as char;
        if ch == '"' && depth == 0 && (idx == 0 || bytes[idx - 1] != b'\\') {
            in_quote = !in_quote;
        }
        if !in_quote {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                if depth > 0 {
                    depth -= 1;
                }
            } else if ch == ',' && depth == 0 {
                break;
            }
        }
        idx += 1;
    }
    let value = body[value_start..idx].trim().to_string();
    if idx < bytes.len() && bytes[idx] == b',' {
        idx += 1;
    }
    (value, idx)
}

fn normalize_bibtex_value(value: &str, string_map: &HashMap<String, String>) -> String {
    let parts = split_bibtex_concat(value);
    let mut out = String::new();
    for part in parts {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let unwrapped = if trimmed.starts_with('{') && trimmed.ends_with('}') && trimmed.len() >= 2
        {
            &trimmed[1..trimmed.len() - 1]
        } else if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
            &trimmed[1..trimmed.len() - 1]
        } else {
            let key = trimmed.to_lowercase();
            if let Some(val) = string_map.get(&key) {
                val
            } else {
                trimmed
            }
        };
        out.push_str(unwrapped);
    }
    out.replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_bibtex_concat(value: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut last = 0usize;
    for (idx, ch) in value.char_indices() {
        if ch == '"' && depth == 0 && (idx == 0 || value.as_bytes()[idx - 1] != b'\\') {
            in_quote = !in_quote;
        }
        if !in_quote {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                if depth > 0 {
                    depth -= 1;
                }
            } else if ch == '#' && depth == 0 {
                parts.push(value[last..idx].to_string());
                last = idx + 1;
            }
        }
    }
    if last <= value.len() {
        parts.push(value[last..].to_string());
    }
    parts
}

/// Strip stars from common sectioning commands (e.g., \chapter* -> \chapter).
pub fn strip_sectioning_stars(input: &str) -> String {
    let mut out = input.to_string();
    for cmd in ["chapter", "section", "subsection", "subsubsection", "part"] {
        let needle = format!("\\{}*", cmd);
        let replacement = format!("\\{}", cmd);
        out = out.replace(&needle, &replacement);
    }
    out
}

/// Strip stars from environment names (e.g. \begin{equation*} -> \begin{equation}).
pub fn strip_env_stars(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && (input[i..].starts_with("\\begin{") || input[i..].starts_with("\\end{"))
        {
            let cmd_len = if input[i..].starts_with("\\begin{") {
                "\\begin{".len()
            } else {
                "\\end{".len()
            };
            out.extend_from_slice(&bytes[i..i + cmd_len]);
            i += cmd_len;
            let start = i;
            while i < bytes.len() && bytes[i] != b'}' {
                i += 1;
            }
            if i < bytes.len() {
                let name = &input[start..i];
                let trimmed = if name.ends_with('*') {
                    &name[..name.len() - 1]
                } else {
                    name
                };
                out.extend_from_slice(trimmed.as_bytes());
                out.push(b'}');
                i += 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Normalize citation commands with two optional arguments into a single optional argument.
/// This helps the parser handle natbib-style \citep[pre][post]{key} constructs.
pub fn normalize_citation_optional_args(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'*') {
                j += 1;
            }
            if j > i + 1 {
                let name = &input[i + 1..j];
                let is_cite = name.starts_with("cite") || name.ends_with("cite");
                if is_cite {
                    let mut k = j;
                    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if k < bytes.len() && bytes[k] == b'[' {
                        if let Some((opt1, end1)) = extract_bracket_arg_at(input, k) {
                            let mut k2 = end1;
                            while k2 < bytes.len() && bytes[k2].is_ascii_whitespace() {
                                k2 += 1;
                            }
                            if k2 < bytes.len() && bytes[k2] == b'[' {
                                if let Some((opt2, end2)) = extract_bracket_arg_at(input, k2) {
                                    out.extend_from_slice(&bytes[i..j]);
                                    out.extend_from_slice(&bytes[j..k]);
                                    let opt1 = opt1.trim();
                                    let opt2 = opt2.trim();
                                    let merged = if opt1.is_empty() && opt2.is_empty() {
                                        String::new()
                                    } else if opt1.is_empty() {
                                        opt2.to_string()
                                    } else if opt2.is_empty() {
                                        opt1.to_string()
                                    } else {
                                        format!("{}; {}", opt1, opt2)
                                    };
                                    if !merged.is_empty() {
                                        out.push(b'[');
                                        out.extend_from_slice(merged.as_bytes());
                                        out.push(b']');
                                    }
                                    i = end2;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Normalize TeX spacing primitives like \hskip and \kern into \hspace{...}.
pub fn normalize_spacing_primitives(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && (input[i..].starts_with("\\hskip")
                || input[i..].starts_with("\\kern")
                || input[i..].starts_with("\\vskip"))
        {
            let (cmd_len, out_cmd) = if input[i..].starts_with("\\hskip") {
                ("\\hskip".len(), b"\\hspace{")
            } else if input[i..].starts_with("\\vskip") {
                ("\\vskip".len(), b"\\vspace{")
            } else {
                ("\\kern".len(), b"\\hspace{")
            };
            let after = i + cmd_len;
            if after < bytes.len() && bytes[after].is_ascii_alphabetic() {
                out.push(bytes[i]);
                i += 1;
                continue;
            }
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let len_start = j;
            while j < bytes.len() && !bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if len_start == j {
                out.extend_from_slice(&bytes[i..after]);
                i = after;
                continue;
            }
            out.extend_from_slice(out_cmd);
            out.extend_from_slice(input[len_start..j].as_bytes());
            out.push(b'}');

            let mut k = j;
            loop {
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k >= bytes.len() {
                    break;
                }
                let rest = &input[k..];
                let kw_len = if rest.starts_with("plus") {
                    4usize
                } else if rest.starts_with("minus") {
                    5usize
                } else {
                    0usize
                };
                if kw_len == 0 {
                    break;
                }
                let next = k + kw_len;
                if next < bytes.len() && bytes[next].is_ascii_alphabetic() {
                    break;
                }
                k = next;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
            }
            i = k;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn extract_bracket_arg_at(input: &str, start: usize) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if start >= bytes.len() || bytes[start] != b'[' {
        return None;
    }
    let mut depth = 0i32;
    let mut idx = start;
    while idx < bytes.len() {
        match bytes[idx] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    let content = input[start + 1..idx].to_string();
                    return Some((content, idx + 1));
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

/// Normalize unmatched display/math delimiters like \] or \) by turning them into literal
/// characters when no corresponding opener has been seen. This prevents parser errors
/// on malformed inputs while preserving valid pairs.
pub fn normalize_math_delimiters(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    let mut square = 0i32;
    let mut round = 0i32;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Copy comments verbatim
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            out.extend_from_slice(&bytes[start..i]);
            continue;
        }

        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            match next {
                b'[' => {
                    square += 1;
                    out.extend_from_slice(b"\\[");
                    i += 2;
                    continue;
                }
                b']' => {
                    if square > 0 {
                        square -= 1;
                        out.extend_from_slice(b"\\]");
                    } else {
                        out.push(b']');
                    }
                    i += 2;
                    continue;
                }
                b'(' => {
                    round += 1;
                    out.extend_from_slice(b"\\(");
                    i += 2;
                    continue;
                }
                b')' => {
                    if round > 0 {
                        round -= 1;
                        out.extend_from_slice(b"\\)");
                    } else {
                        out.push(b')');
                    }
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Normalize $$ ... $$ display math to \[ ... \] pairs.
pub fn normalize_display_dollars(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    let mut open = false;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            out.extend_from_slice(&bytes[start..i]);
            continue;
        }

        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            out.extend_from_slice(b"\\$");
            i += 2;
            continue;
        }

        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            if open {
                out.extend_from_slice(b"\\]");
            } else {
                out.extend_from_slice(b"\\[");
            }
            open = !open;
            i += 2;
            continue;
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Collapse double-dollar sequences in Typst output to single dollars.
pub fn normalize_typst_double_dollars(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            out.push(b'\\');
            if i + 1 < bytes.len() {
                out.push(bytes[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'$' {
            out.push(b'$');
            i += 2;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Replace #linebreak() with a plain space to avoid math parse errors.
pub fn normalize_typst_linebreaks(input: &str) -> String {
    input.replace("#linebreak()", " ")
}

/// Normalize `op([...])` patterns in math output to plain parentheses.
pub fn normalize_typst_op_brackets(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0usize;
    let mut in_string = false;
    let mut op_depth = 0i32;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_string = !in_string;
            out.push(b'"');
            i += 1;
            continue;
        }
        if !in_string && i + 3 < bytes.len() && &bytes[i..i + 4] == b"op([" {
            out.push(b'(');
            i += 4;
            op_depth += 1;
            continue;
        }
        if !in_string && op_depth > 0 && bytes[i] == b']' {
            if i + 1 < bytes.len() && bytes[i + 1] == b')' {
                out.push(b')');
                i += 2;
                op_depth -= 1;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Drop unmatched closing braces to avoid parser errors after macro expansion.
pub fn normalize_unmatched_braces(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut depth = 0i32;
    let mut i = 0usize;
    let mut in_comment = false;

    while i < bytes.len() {
        if in_comment {
            if bytes[i] == b'\n' {
                in_comment = false;
            }
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if bytes[i] == b'%' {
            in_comment = true;
            out.push(b'%');
            i += 1;
            continue;
        }
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'{' || next == b'}' {
                out.push(b'\\');
                out.push(next);
                i += 2;
                continue;
            }
        }

        match bytes[i] {
            b'{' => {
                depth += 1;
                out.push(b'{');
            }
            b'}' => {
                if depth > 0 {
                    depth -= 1;
                    out.push(b'}');
                } else {
                    // Drop unmatched closing brace
                }
            }
            _ => out.push(bytes[i]),
        }
        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Strip optional bracket arguments from specific environments (e.g., nomenclature).
pub fn strip_env_options(input: &str, envs: &[&str]) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 7 <= bytes.len() && &bytes[i..i + 7] == b"\\begin{" {
            let start = i;
            let mut j = i + 7;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() {
                if let Ok(env_name) = std::str::from_utf8(&bytes[i + 7..j]) {
                    if envs.iter().any(|e| *e == env_name) {
                        let end = j + 1;
                        out.extend_from_slice(&bytes[start..end]);
                        i = end;
                        // Skip optional bracket groups after \begin{env}
                        loop {
                            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                                out.push(bytes[i]);
                                i += 1;
                            }
                            if i < bytes.len() && bytes[i] == b'[' {
                                let mut depth = 0i32;
                                let mut k = i;
                                while k < bytes.len() {
                                    match bytes[k] {
                                        b'[' => depth += 1,
                                        b']' => {
                                            depth -= 1;
                                            if depth == 0 {
                                                k += 1;
                                                break;
                                            }
                                        }
                                        _ => {}
                                    }
                                    k += 1;
                                }
                                i = k;
                                continue;
                            }
                            break;
                        }
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Strip optional bracket arguments from specific commands (e.g., \blindtext[2]).
pub fn strip_command_optional_arg(input: &str, commands: &[&str]) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            for cmd in commands {
                let needle = cmd.as_bytes();
                if i + 1 + needle.len() <= bytes.len()
                    && &bytes[i + 1..i + 1 + needle.len()] == needle
                {
                    let mut j = i + 1 + needle.len();
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        out.push(bytes[j]);
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'[' {
                        let mut depth = 0i32;
                        let mut k = j;
                        while k < bytes.len() {
                            match bytes[k] {
                                b'[' => depth += 1,
                                b']' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        k += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            k += 1;
                        }
                        out.extend_from_slice(&bytes[i..i + 1 + needle.len()]);
                        i = k;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Replace inline math blocks that only contain a superscript (e.g., $^{th}$) with \textsuperscript{...}.
pub fn replace_empty_math_superscripts(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if i + 1 < bytes.len() {
                out.push(bytes[i]);
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'$' {
            if i > 0 && bytes[i - 1] == b'\\' {
                out.push(b'$');
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < bytes.len() {
                if bytes[j] == b'$' && bytes[j - 1] != b'\\' {
                    break;
                }
                j += 1;
            }
            if j < bytes.len() && j > i + 1 {
                let inner = &input[i + 1..j];
                let trimmed = inner.trim_start();
                if trimmed.starts_with('^') {
                    let mut content = trimmed.trim_start_matches('^').trim();
                    if content.starts_with('{') && content.ends_with('}') && content.len() >= 2 {
                        content = &content[1..content.len() - 1];
                        content = content.trim();
                    }
                    out.extend_from_slice(b"\\textsuperscript{");
                    out.extend_from_slice(content.as_bytes());
                    out.push(b'}');
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Avoid accidental "*/" sequences before loss markers (e.g. "*/* tylax:loss:" -> "* /* tylax:loss:")
pub fn sanitize_loss_comment_boundaries(input: &str) -> String {
    input.replace("*/* tylax:loss:", "* /* tylax:loss:")
}

// =============================================================================
// Math Cleanup Helpers
// =============================================================================

/// Wrap a base expression with limits(...) unless it's already a limits() call.
/// Returns None when the base is empty after trimming.
pub fn wrap_with_limits_for_stack(base: &str) -> Option<String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("limits(") {
        return Some(trimmed.to_string());
    }
    Some(format!("limits({})", trimmed))
}

/// Merge "arg" followed by "min"/"max" into a single operator.
/// Returns true if a merge happened and output was updated.
pub fn merge_arg_operator(output: &mut String, tail: &str) -> bool {
    let trimmed_len = output.trim_end().len();
    let prefix = &output[..trimmed_len];

    if !prefix.ends_with("arg") {
        return false;
    }

    let before = &prefix[..prefix.len() - 3];
    let prev = before.chars().rev().find(|c| !c.is_whitespace());
    if let Some(ch) = prev {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            return false;
        }
    }

    output.truncate(prefix.len() - 3);
    let _ = write!(output, "op(\"arg{}\") ", tail);
    true
}

/// Wrap the trailing operator with limits(...).
/// Returns true if an operator was wrapped.
pub fn apply_limits_to_trailing_operator(output: &mut String) -> bool {
    let trimmed_len = output.trim_end().len();
    if trimmed_len == 0 {
        return false;
    }

    let prefix = &output[..trimmed_len];
    if prefix.ends_with("limits)") || prefix.ends_with("limits )") {
        return false;
    }

    if let Some(expr_start) = find_trailing_op_call(prefix) {
        let expr = prefix[expr_start..].to_string();
        output.truncate(expr_start);
        let _ = write!(output, "limits({}) ", expr);
        return true;
    }

    const OPS: [&str; 24] = [
        "sum",
        "product",
        "integral",
        "integral.double",
        "integral.triple",
        "integral.cont",
        "lim",
        "sup",
        "inf",
        "max",
        "min",
        "argmin",
        "argmax",
        "det",
        "gcd",
        "lcm",
        "union.big",
        "sect.big",
        "plus.circle.big",
        "times.circle.big",
        "union.sq.big",
        "union.plus.big",
        "or.big",
        "and.big",
    ];

    for op in OPS {
        if !prefix.ends_with(op) {
            continue;
        }
        let before = &prefix[..prefix.len() - op.len()];
        let prev = before.chars().rev().find(|c| !c.is_whitespace());
        if let Some(ch) = prev {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                continue;
            }
        }

        output.truncate(prefix.len() - op.len());
        let _ = write!(output, "limits({}) ", op);
        return true;
    }

    false
}

fn find_trailing_op_call(s: &str) -> Option<usize> {
    let trimmed = s.trim_end();
    if !trimmed.ends_with(')') {
        return None;
    }

    let mut depth = 0i32;
    let mut open_idx = None;
    for (idx, ch) in trimmed.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    open_idx = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }

    let open_idx = open_idx?;
    let func = trimmed[..open_idx].trim_end();
    if func.ends_with("limits") {
        return None;
    }
    if !func.ends_with("op") {
        return None;
    }

    let func_start = func.len() - 2;
    let before = func[..func_start]
        .chars()
        .rev()
        .find(|c| !c.is_whitespace());
    if let Some(ch) = before {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            return None;
        }
    }
    Some(func_start)
}

/// Format a basic chemical formula for Typst math (e.g., H2O -> upright(H_2O)).
pub fn format_chemical_formula_math(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let mut chars = trimmed.chars().peekable();
    let mut prev_non_space: Option<char> = None;

    while let Some(ch) = chars.next() {
        if ch.is_ascii_whitespace() {
            out.push(' ');
            continue;
        }

        if ch.is_ascii_digit() {
            let mut digits = String::new();
            digits.push(ch);
            while let Some(next) = chars.peek().copied() {
                if next.is_ascii_digit() {
                    digits.push(next);
                    chars.next();
                } else {
                    break;
                }
            }

            let use_subscript = matches!(
                prev_non_space,
                Some(p) if p.is_ascii_alphabetic() || p == ')' || p == ']' || p == '}'
            );
            if use_subscript {
                let _ = write!(out, "_({})", digits);
            } else {
                out.push_str(&digits);
            }
            prev_non_space = Some(ch);
            continue;
        }

        if ch.is_ascii_alphabetic() {
            if let Some(prev) = prev_non_space {
                if prev.is_ascii_alphabetic() {
                    out.push(' ');
                }
            }
        }

        out.push(ch);
        prev_non_space = Some(ch);
    }

    format!("upright({})", out)
}

/// Sanitize mhchem-style content for safe insertion into text() in math mode.
/// Drops LaTeX command markers and math delimiters to avoid invalid Typst syntax.
pub fn sanitize_ce_text_for_math(raw: &str) -> String {
    fn consume_braced_content(
        chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    ) -> Option<String> {
        while matches!(chars.peek(), Some(' ')) {
            chars.next();
        }
        if chars.peek() != Some(&'{') {
            return None;
        }
        chars.next();
        let mut content = String::new();
        let mut depth = 1i32;
        for ch in chars.by_ref() {
            match ch {
                '{' => {
                    depth += 1;
                    content.push(ch);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    content.push(ch);
                }
                _ => content.push(ch),
            }
        }
        Some(content)
    }

    fn is_text_cmd(cmd: &str) -> bool {
        matches!(
            cmd,
            "text"
                | "textrm"
                | "textup"
                | "textnormal"
                | "textit"
                | "textbf"
                | "textsf"
                | "texttt"
                | "textsc"
                | "mathrm"
                | "mathit"
                | "mathbf"
                | "mathsf"
                | "mathbb"
                | "mathcal"
                | "mathfrak"
        )
    }

    let mut out = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '$' => {
                let mut inner = String::new();
                while let Some(next) = chars.next() {
                    if next == '$' {
                        break;
                    }
                    inner.push(next);
                }
                let inner_clean = sanitize_ce_text_for_math(&inner);
                out.push_str(&inner_clean);
            }
            '\\' => {
                let mut cmd = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_alphabetic() {
                        cmd.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if cmd.is_empty() {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                    continue;
                }

                let key = format!("\\{}", cmd);
                if let Some(symbol) = GREEK_LETTERS.get(key.as_str()) {
                    out.push_str(symbol);
                    continue;
                }

                if cmd == "ce" {
                    if let Some(content) = consume_braced_content(&mut chars) {
                        out.push_str(&sanitize_ce_text_for_math(&content));
                    }
                    continue;
                }

                if cmd == "underset" || cmd == "overset" || cmd == "stackrel" {
                    let first = consume_braced_content(&mut chars).unwrap_or_default();
                    let second = consume_braced_content(&mut chars).unwrap_or_default();
                    let base = sanitize_ce_text_for_math(&second);
                    let annotation = sanitize_ce_text_for_math(&first);
                    if !base.trim().is_empty() {
                        out.push_str(base.trim());
                    }
                    if !annotation.trim().is_empty() {
                        if !out.ends_with(' ') && !out.is_empty() {
                            out.push(' ');
                        }
                        out.push_str(annotation.trim());
                    }
                    continue;
                }

                if is_text_cmd(&cmd) {
                    if let Some(content) = consume_braced_content(&mut chars) {
                        out.push_str(&sanitize_ce_text_for_math(&content));
                    }
                    continue;
                }

                out.push_str(&cmd);
            }
            '{' | '}' => {}
            _ => out.push(ch),
        }
    }

    out.trim().to_string()
}

/// Remove unescaped '$' characters from text destined for math text literals.
pub fn strip_unescaped_dollars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut backslashes = 0usize;

    for ch in input.chars() {
        if ch == '\\' {
            backslashes += 1;
            out.push(ch);
            continue;
        }

        if ch == '$' && backslashes % 2 == 0 {
            backslashes = 0;
            continue;
        }

        out.push(ch);
        backslashes = 0;
    }

    out
}

fn convert_string_entries(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            // Check for @string( pattern using bytes to avoid char boundary issues
            let remaining = bytes.len() - i;
            if remaining >= 8
                && bytes[i..i + 8]
                    .iter()
                    .zip(b"@string(")
                    .all(|(a, b)| a.eq_ignore_ascii_case(b))
            {
                out.extend_from_slice(b"@string{");
                let mut j = i + 8;
                let mut depth = 1usize;
                while j < bytes.len() {
                    match bytes[j] {
                        b'(' => depth += 1,
                        b')' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                // Use bytes directly to avoid char boundary issues
                                out.extend_from_slice(&bytes[i + 8..j]);
                                out.push(b'}');
                                i = j + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if j >= bytes.len() {
                    out.extend_from_slice(&bytes[i + 8..]);
                    break;
                }
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn sanitize_bibtex_keys(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            // Only treat '@' as an entry start if it appears at line start (ignoring spaces).
            let mut k = i;
            while k > 0 && bytes[k - 1].is_ascii_whitespace() && bytes[k - 1] != b'\n' {
                k -= 1;
            }
            let at_line_start = k == 0 || bytes[k - 1] == b'\n';
            if !at_line_start {
                out.push(b'@');
                i += 1;
                continue;
            }
            let start = i;
            i += 1;
            let mut entry_type = String::new();
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                entry_type.push(bytes[i] as char);
                i += 1;
            }
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'{' || bytes[j] == b'(') {
                let open = bytes[j] as char;
                j += 1;
                let entry_type_lower = entry_type.to_lowercase();
                if entry_type_lower == "string"
                    || entry_type_lower == "preamble"
                    || entry_type_lower == "comment"
                {
                    // Preserve meta entries as-is to avoid corrupting their content.
                    let close = if open == '{' { '}' } else { ')' };
                    let mut depth = 1i32;
                    while j < bytes.len() && depth > 0 {
                        let ch = bytes[j] as char;
                        if ch == open {
                            depth += 1;
                        } else if ch == close {
                            depth -= 1;
                        }
                        j += 1;
                    }
                    let end = j.min(bytes.len());
                    out.extend_from_slice(input[start..end].as_bytes());
                    i = end;
                    continue;
                }
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let key_start = j;
                while j < bytes.len() && bytes[j] != b',' && bytes[j] != b'\n' && bytes[j] != b'\r'
                {
                    j += 1;
                }
                let key_raw = input[key_start..j].trim();
                let mut k = j;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b',' {
                    let sanitized = sanitize_citation_key(key_raw);
                    out.push(b'@');
                    out.extend_from_slice(entry_type.as_bytes());
                    out.push(open as u8);
                    out.extend_from_slice(sanitized.as_bytes());
                    out.push(b',');
                    i = k + 1;
                    continue;
                }
            }
            out.extend_from_slice(input[start..i].as_bytes());
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Escape plain text for Typst markup.
/// This is applied to non-math text tokens to avoid accidental markup (e.g., emails, underscores).
pub fn escape_typst_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '@' | '_' | '*' | '#' | '$' | '`' | '<' | '>' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Unescape common LaTeX escaped characters inside monospace/raw contexts like \texttt{...}.
/// We keep unknown commands intact to avoid losing intentional literals.
pub fn unescape_latex_monospace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let Some(&next) = chars.peek() else {
            out.push('\\');
            break;
        };

        match next {
            // Double backslash (\\ in LaTeX) becomes single backslash
            '\\' => {
                out.push('\\');
                chars.next();
            }
            '{' | '}' | '%' | '#' | '&' | '_' | '$' | '@' => {
                out.push(next);
                chars.next();
            }
            '~' => {
                chars.next();
                // Consume optional empty braces: \~{} -> ~
                if chars.peek() == Some(&'{') {
                    chars.next();
                    if chars.peek() == Some(&'}') {
                        chars.next();
                    }
                }
                out.push('~');
            }
            '^' => {
                chars.next();
                // Consume optional empty braces: \^{} -> ^
                if chars.peek() == Some(&'{') {
                    chars.next();
                    if chars.peek() == Some(&'}') {
                        chars.next();
                    }
                }
                out.push('^');
            }
            _ if next.is_ascii_alphabetic() => {
                let mut cmd = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        cmd.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match cmd.as_str() {
                    "textbackslash" => {
                        // Optional empty braces: \textbackslash{}
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                        }
                        out.push('\\');
                    }
                    "textasciitilde" => {
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                        }
                        out.push('~');
                    }
                    "textasciicircum" => {
                        if chars.peek() == Some(&'{') {
                            chars.next();
                            if chars.peek() == Some(&'}') {
                                chars.next();
                            }
                        }
                        out.push('^');
                    }
                    _ => {
                        out.push('\\');
                        out.push_str(&cmd);
                    }
                }
            }
            _ => {
                out.push('\\');
                out.push(next);
                chars.next();
            }
        }
    }
    out
}

/// Escape plain text for Typst markup into an existing buffer.
pub fn escape_typst_text_into(text: &str, out: &mut String) {
    if !text
        .as_bytes()
        .iter()
        .any(|b| matches!(b, b'@' | b'_' | b'*' | b'#' | b'$' | b'`' | b'<' | b'>'))
    {
        out.push_str(text);
        return;
    }
    for ch in text.chars() {
        match ch {
            '@' | '_' | '*' | '#' | '$' | '`' | '<' | '>' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
}

/// Escape content for Typst string literals.
pub fn escape_typst_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "")
}

/// Strip a \label{...} command from raw text and return (clean_text, label).
pub fn strip_label_from_text(raw: &str) -> (String, Option<String>) {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut label: Option<String> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && raw[i..].starts_with("\\label") {
            let mut j = i + "\\label".len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let mut depth = 0i32;
                let mut end = None;
                for (off, ch) in raw[j..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(j + off);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(end_pos) = end {
                    let content = raw[j + 1..end_pos].trim();
                    if !content.is_empty() {
                        label = Some(content.to_string());
                    }
                    i = end_pos + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let cleaned = String::from_utf8(out).unwrap_or_else(|_| raw.to_string());
    let cleaned = cleaned.trim().to_string();
    let cleaned_label = label
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    (cleaned, cleaned_label)
}

/// Escape '@' occurrences that are not valid Typst references or citations.
pub fn escape_at_in_words(input: &str) -> String {
    let labels = collect_emitted_labels(input);
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0usize;
    let mut in_string = false;
    let mut string_escape = false;

    while i < len {
        let ch = chars[i];
        if in_string {
            out.push(ch);
            if string_escape {
                string_escape = false;
            } else if ch == '\\' {
                string_escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '@' {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let prev_is_escape = prev == Some('\\');
            let mut k = i;
            while k > 0 && chars[k - 1].is_whitespace() {
                k -= 1;
            }
            let prev_nonspace = if k > 0 { Some(chars[k - 1]) } else { None };
            let prev_is_cite = matches!(prev_nonspace, Some('[') | Some(';') | Some(','));

            // Extract candidate label after '@'
            let mut j = i + 1;
            while j < len && (chars[j].is_ascii_alphanumeric() || chars[j] == '-') {
                j += 1;
            }
            let candidate: String = chars[i + 1..j].iter().collect();

            if !prev_is_escape
                && !prev_is_cite
                && !candidate.is_empty()
                && !labels.contains(&candidate)
            {
                out.push('\\');
                out.push('@');
                i += 1;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

/// Normalize LaTeX-style quotes to plain double quotes.
pub fn normalize_latex_quotes(input: &str) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut in_code_block = false;
    let mut in_inline_raw = false;
    let mut in_string = false;
    let mut in_math = false;

    for line in input.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            out.extend_from_slice(line.as_bytes());
            continue;
        }
        if in_code_block {
            out.extend_from_slice(line.as_bytes());
            continue;
        }

        let bytes = line.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            let ch = bytes[i];
            if ch == b'$' && !in_inline_raw && !in_string {
                let prev_is_escape = i > 0 && bytes[i - 1] == b'\\';
                if !prev_is_escape {
                    in_math = !in_math;
                }
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == b'\\' {
                if !in_inline_raw && !in_string && !in_math && i + 2 < bytes.len() {
                    if bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
                        out.push(b'"');
                        i += 3;
                        continue;
                    }
                    if bytes[i + 1] == b'\'' && bytes[i + 2] == b'\'' {
                        out.push(b'"');
                        i += 3;
                        continue;
                    }
                }
                out.push(b'\\');
                if i + 1 < bytes.len() {
                    out.push(bytes[i + 1]);
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }

            if !in_inline_raw && !in_string && !in_math {
                if ch == b'`' && i + 1 < bytes.len() && bytes[i + 1] == b'`' {
                    out.push(b'"');
                    i += 2;
                    continue;
                }
                if ch == b'\'' && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    out.push(b'"');
                    i += 2;
                    continue;
                }
            }

            if ch == b'`' && !in_string {
                in_inline_raw = !in_inline_raw;
                out.push(b'`');
                i += 1;
                continue;
            }

            if ch == b'"' && !in_inline_raw {
                in_string = !in_string;
                out.push(b'"');
                i += 1;
                continue;
            }

            out.push(ch);
            i += 1;
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Replace \verb and \lstinline delimiters with a brace-based form so the parser can handle it.
pub fn replace_verb_commands(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Handle \verb and \lstinline commands
            let (cmd_len, target_cmd) = if input[i..].starts_with("\\lstinline") {
                (10, "lstinline")
            } else if input[i..].starts_with("\\verb") {
                (5, "texttt")
            } else {
                (0, "")
            };

            if cmd_len > 0 {
                let mut j = i + cmd_len;
                // Skip optional star
                if j < bytes.len() && bytes[j] == b'*' {
                    j += 1;
                }
                // Skip whitespace
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= bytes.len() {
                    out.extend_from_slice(input[i..].as_bytes());
                    break;
                }

                // Handle brace-delimited content
                if bytes[j] == b'{' {
                    // Find matching close brace (handle nesting)
                    let start = j + 1;
                    let mut depth = 1;
                    j = start;
                    while j < bytes.len() && depth > 0 {
                        if bytes[j] == b'{' {
                            depth += 1;
                        } else if bytes[j] == b'}' {
                            depth -= 1;
                        }
                        if depth > 0 {
                            j += 1;
                        }
                    }
                    if j >= bytes.len() && depth > 0 {
                        out.extend_from_slice(input[i..].as_bytes());
                        break;
                    }
                    let content = &input[start..j];
                    // Protect underscores and special chars
                    let escaped: String = content
                        .chars()
                        .map(|ch| match ch {
                            '_' => "\\_".to_string(),
                            '{' => "\\{".to_string(),
                            '}' => "\\}".to_string(),
                            '#' => "\\#".to_string(),
                            '%' => "\\%".to_string(),
                            '&' => "\\&".to_string(),
                            _ => ch.to_string(),
                        })
                        .collect();
                    out.extend_from_slice(format!("\\{target_cmd}{{").as_bytes());
                    out.extend_from_slice(escaped.as_bytes());
                    out.push(b'}');
                    i = j + 1;
                    continue;
                }

                // Handle arbitrary delimiter (like \verb|...|)
                let delim = bytes[j];
                j += 1;
                let start = j;
                while j < bytes.len() && bytes[j] != delim {
                    j += 1;
                }
                if j >= bytes.len() {
                    out.extend_from_slice(input[i..].as_bytes());
                    break;
                }
                let content = &input[start..j];
                let mut escaped = String::with_capacity(content.len());
                for ch in content.chars() {
                    match ch {
                        '_' => escaped.push_str("\\_"),
                        '{' => escaped.push_str("\\{"),
                        '}' => escaped.push_str("\\}"),
                        '#' => escaped.push_str("\\#"),
                        '%' => escaped.push_str("\\%"),
                        '&' => escaped.push_str("\\&"),
                        _ => escaped.push(ch),
                    }
                }
                out.extend_from_slice(format!("\\{target_cmd}{{").as_bytes());
                out.extend_from_slice(escaped.as_bytes());
                out.push(b'}');
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Collect all \label{...} occurrences from LaTeX source.
pub fn collect_labels(input: &str) -> HashSet<String> {
    let mut labels = HashSet::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip comments
        if bytes[i] == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'\\' && input[i..].starts_with("\\label") {
            let after = i + 6;
            if after < bytes.len() && bytes[after].is_ascii_alphabetic() {
                i += 1;
                continue;
            }
            let mut j = after;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'{' {
                let (content, consumed) = extract_braced_content(&input[j..]);
                if let Some(label) = content {
                    labels.insert(sanitize_label(label.trim()));
                    i = j + consumed;
                    continue;
                }
            }
        }
        i += 1;
    }
    labels
}

fn extract_braced_content(input: &str) -> (Option<String>, usize) {
    let mut depth = 0usize;
    let mut start = None;
    for (idx, ch) in input.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                if depth == 1 {
                    start = Some(idx + 1);
                }
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start {
                            return (Some(input[s..idx].to_string()), idx + 1);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (None, 0)
}

fn parse_braced_groups(input: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    for (idx, ch) in input.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(idx + 1);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start {
                            groups.push(input[s..idx].to_string());
                        }
                        start = None;
                    }
                }
            }
            _ => {}
        }
    }
    groups
}

/// Resolve reference placeholders emitted during conversion using labels present in the output.
pub fn resolve_reference_markers(input: &str) -> String {
    const REF_MARKER: &str = "__TYLAX_REF__";
    const EQREF_MARKER: &str = "__TYLAX_EQREF__";
    const PAGEREF_MARKER: &str = "__TYLAX_PAGEREF__";

    let labels = collect_emitted_labels(input);

    let mut out = replace_marker(input, REF_MARKER, |label| {
        if labels.contains(label) {
            format!("@{}", label)
        } else {
            format!("\\@{}", label)
        }
    });
    out = replace_marker(&out, EQREF_MARKER, |label| {
        if labels.contains(label) {
            format!("(@{})", label)
        } else {
            format!("(\\@{})", label)
        }
    });
    out = replace_marker(&out, PAGEREF_MARKER, |label| {
        if labels.contains(label) {
            format!("#context locate(<{}>).page()", label)
        } else {
            format!("\\@{}", label)
        }
    });
    out
}

/// Attach standalone label lines to the previous non-empty line.
pub fn attach_orphan_labels(input: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut last_content_idx: Option<usize> = None;

    for line in input.lines() {
        let trimmed = line.trim();
        if is_label_only(trimmed) {
            if let Some(idx) = last_content_idx {
                let prev_raw = lines[idx].trim_end().to_string();
                let mut merged = prev_raw.clone();
                if !merged.ends_with('>') {
                    if let Some(last) = prev_raw.chars().last() {
                        if last == '_' || last == '*' {
                            let count = prev_raw.chars().filter(|c| *c == last).count();
                            if count % 2 == 1 {
                                // Likely opening emphasis marker - insert label before it.
                                let core = &prev_raw[..prev_raw.len() - 1];
                                let tail = &prev_raw[prev_raw.len() - 1..];
                                merged = format!("{} {}{}", core, trimmed, tail);
                                lines[idx] = merged;
                                continue;
                            }
                        }
                    }
                    merged.push(' ');
                    merged.push_str(trimmed);
                    lines[idx] = merged;
                } else {
                    lines.push(line.to_string());
                }
            } else {
                lines.push(line.to_string());
            }
        } else {
            if !trimmed.is_empty() {
                last_content_idx = Some(lines.len());
            }
            lines.push(line.to_string());
        }
    }

    lines.join("\n")
}

fn is_label_only(s: &str) -> bool {
    s.starts_with('<')
        && s.ends_with('>')
        && !s.contains(' ')
        && s.len() > 2
        && s[1..s.len() - 1]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn collect_emitted_labels(input: &str) -> HashSet<String> {
    let mut labels = HashSet::new();
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'>' && !bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'>' && j > i + 1 {
                let candidate = &input[i + 1..j];
                if candidate
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                {
                    labels.insert(candidate.to_string());
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    labels
}

fn replace_marker<F>(input: &str, marker: &str, mut f: F) -> String
where
    F: FnMut(&str) -> String,
{
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while let Some(pos) = input[i..].find(marker) {
        let start = i + pos;
        out.push_str(&input[i..start]);
        let label_start = start + marker.len();
        if let Some(end_rel) = input[label_start..].find("__") {
            let label_end = label_start + end_rel;
            let label = &input[label_start..label_end];
            out.push_str(&f(label));
            i = label_end + 2;
        } else {
            out.push_str(&input[start..]);
            return out;
        }
    }
    out.push_str(&input[i..]);
    out
}

/// Expand \input{...} and \include{...} directives using the filesystem.
pub fn expand_latex_inputs(input: &str, base_dir: &std::path::Path) -> String {
    let mut seen = HashSet::new();
    expand_latex_inputs_inner(input, base_dir, 0, &mut seen)
}

/// Collect package names from \usepackage / \RequirePackage commands.
pub fn collect_usepackage_entries(input: &str) -> Vec<String> {
    let stripped = strip_latex_comments(input);
    let mut packages = Vec::new();
    let bytes = stripped.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let remaining = &stripped[i..];
            let (_cmd, cmd_len) = if remaining.starts_with("\\usepackage") {
                ("\\usepackage", 11usize)
            } else if remaining.starts_with("\\RequirePackage") {
                ("\\RequirePackage", 15usize)
            } else {
                ("", 0usize)
            };
            if cmd_len > 0 {
                if i + cmd_len < bytes.len() && bytes[i + cmd_len].is_ascii_alphabetic() {
                    i += 1;
                    continue;
                }
                let mut j = i + cmd_len;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                // Optional [..] options
                if j < bytes.len() && bytes[j] == b'[' {
                    let mut depth = 1usize;
                    j += 1;
                    while j < bytes.len() && depth > 0 {
                        match bytes[j] {
                            b'[' => depth += 1,
                            b']' => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        j += 1;
                    }
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                }
                if j < bytes.len() && bytes[j] == b'{' {
                    let (content, used) = extract_braced_content(&stripped[j..]);
                    if let Some(content) = content {
                        for part in content.split(',') {
                            let trimmed = part.trim();
                            if !trimmed.is_empty() {
                                packages.push(trimmed.to_string());
                            }
                        }
                    }
                    i = j + used;
                    continue;
                }
            }
        }
        i += 1;
    }
    packages
}

/// Strip LaTeX line comments (% ...) while preserving escaped \%.
pub fn strip_latex_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let mut prev_backslash = false;
        for ch in line.chars() {
            if ch == '%' && !prev_backslash {
                break;
            }
            out.push(ch);
            prev_backslash = ch == '\\';
        }
        out.push('\n');
    }
    out
}

/// Expand local \usepackage / \RequirePackage files found on disk.
/// Only includes package files that exist under base_dir.
pub fn expand_local_packages(input: &str, base_dir: &std::path::Path) -> String {
    expand_local_packages_with_skip(input, base_dir, &HashSet::new())
}

/// Expand local \usepackage / \RequirePackage files found on disk, skipping any
/// package names present in `skip_packages` (case-insensitive).
pub fn expand_local_packages_with_skip(
    input: &str,
    base_dir: &std::path::Path,
    skip_packages: &HashSet<String>,
) -> String {
    let mut seen = HashSet::new();
    let mut expanded = String::new();
    for pkg in collect_usepackage_entries(input) {
        let pkg_trimmed = pkg.trim();
        if pkg_trimmed.is_empty() {
            continue;
        }
        let pkg_lower = pkg_trimmed.to_lowercase();
        if skip_packages.contains(&pkg_lower) {
            continue;
        }
        let path = std::path::PathBuf::from(pkg_trimmed);
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if path.extension().is_none() {
            let mut sty = path.clone();
            sty.set_extension("sty");
            candidates.push(sty);
            let mut tex = path.clone();
            tex.set_extension("tex");
            candidates.push(tex);
        } else {
            candidates.push(path.clone());
        }
        for cand in candidates {
            let full_path = if cand.is_absolute() {
                cand
            } else {
                base_dir.join(&cand)
            };
            if !full_path.exists() || seen.contains(&full_path) {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                seen.insert(full_path.clone());
                expanded.push_str("% --- local package: ");
                expanded.push_str(full_path.to_string_lossy().as_ref());
                expanded.push_str(" ---\n");
                expanded.push_str("\\makeatletter\n");
                expanded.push_str(&content);
                expanded.push_str("\n\\makeatother\n");
            }
        }
    }
    if expanded.is_empty() {
        input.to_string()
    } else {
        format!("{}\n{}", expanded, input)
    }
}

fn expand_latex_inputs_inner(
    input: &str,
    base_dir: &std::path::Path,
    depth: usize,
    seen: &mut HashSet<std::path::PathBuf>,
) -> String {
    const MAX_DEPTH: usize = 12;
    if depth > MAX_DEPTH {
        return input.to_string();
    }

    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let remaining = &input[i..];
            let (_cmd, cmd_len) = if remaining.starts_with("\\input") {
                ("\\input", 6usize)
            } else if remaining.starts_with("\\include") {
                ("\\include", 8usize)
            } else {
                ("", 0usize)
            };
            if cmd_len > 0 {
                // Avoid matching commands like \inputenc
                if i + cmd_len < bytes.len() && bytes[i + cmd_len].is_ascii_alphabetic() {
                    out.push(bytes[i]);
                    i += 1;
                    continue;
                }
                let mut j = i + cmd_len;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let mut path_str = String::new();
                let mut end_idx = 0usize;
                if j < bytes.len() && bytes[j] == b'{' {
                    let (content, used) = extract_braced_content(&input[j..]);
                    if let Some(c) = content {
                        path_str = c;
                        end_idx = j + used;
                    }
                } else {
                    let start = j;
                    while j < bytes.len() && !bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j > start {
                        path_str = input[start..j].to_string();
                        end_idx = j;
                    }
                }

                if !path_str.is_empty() {
                    let mut path = std::path::PathBuf::from(path_str.trim());
                    if path.extension().is_none() {
                        path.set_extension("tex");
                    }
                    let full_path = if path.is_absolute() {
                        path
                    } else {
                        base_dir.join(path)
                    };

                    if seen.contains(&full_path) {
                        if end_idx > i {
                            out.extend_from_slice(input[i..end_idx].as_bytes());
                            i = end_idx;
                            continue;
                        }
                    }

                    if let Ok(content) = std::fs::read_to_string(&full_path) {
                        seen.insert(full_path.clone());
                        let next_base = full_path.parent().unwrap_or(base_dir);
                        let expanded =
                            expand_latex_inputs_inner(&content, next_base, depth + 1, seen);
                        out.extend_from_slice(expanded.as_bytes());
                        if end_idx > 0 {
                            i = end_idx;
                        } else {
                            i = j;
                        }
                        continue;
                    } else if end_idx > i {
                        out.extend_from_slice(input[i..end_idx].as_bytes());
                        i = end_idx;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// Strip occurrences of \command{...} (single braced argument) from LaTeX source.
pub fn strip_command_with_braced_arg(input: &str, cmd: &str) -> String {
    let needle = format!("\\{}", cmd);
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' {
            let remaining: String = chars[i..].iter().collect();
            if remaining.starts_with(&needle) {
                let mut j = i + needle.len();
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '{' {
                    let mut depth = 1usize;
                    j += 1;
                    while j < chars.len() && depth > 0 {
                        match chars[j] {
                            '{' => depth += 1,
                            '}' => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Replace occurrences of \coloremojicode{HEX} with "emoji-HEX".
pub fn replace_coloremojicode(input: &str) -> String {
    let needle = "\\coloremojicode";
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '\\' {
            let remaining: String = chars[i..].iter().collect();
            if remaining.starts_with(needle) {
                let mut j = i + needle.len();
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '{' {
                    let mut depth = 1usize;
                    let mut content = String::new();
                    j += 1;
                    while j < chars.len() && depth > 0 {
                        match chars[j] {
                            '{' => {
                                depth += 1;
                                content.push(chars[j]);
                            }
                            '}' => {
                                depth = depth.saturating_sub(1);
                                if depth > 0 {
                                    content.push(chars[j]);
                                }
                            }
                            _ => content.push(chars[j]),
                        }
                        j += 1;
                    }
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        out.push_str("emoji-");
                        out.push_str(trimmed);
                    }
                    i = j;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Convert integer to Roman numeral
pub fn to_roman_numeral(num: usize) -> String {
    if num == 0 {
        return "0".to_string();
    }

    let values = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut result = String::new();
    let mut n = num;

    for (value, symbol) in values {
        while n >= value {
            result.push_str(symbol);
            n -= value;
        }
    }

    result
}

// =============================================================================
// Command Protection/Restoration
// =============================================================================

/// Protect zero-argument commands from being lost during parsing.
/// Replaces specific commands with Unicode private use area placeholders that survive the MiTeX parser.
pub fn protect_zero_arg_commands(input: &str) -> String {
    let mut result = input.to_string();
    // Use text placeholders wrapped in Private Use Area characters to avoid parser interference.
    result = result.replace("\\today", "\u{E000}TODAY\u{E001}");
    result = result.replace("\\LaTeX", "\u{E000}LATEX\u{E001}");
    result = result.replace("\\TeX", "\u{E000}TEX\u{E001}");
    result = result.replace("\\XeTeX", "\u{E000}XETEX\u{E001}");
    result = result.replace("\\LuaTeX", "\u{E000}LUATEX\u{E001}");
    result = result.replace("\\pdfTeX", "\u{E000}PDFTEX\u{E001}");
    result = result.replace("\\BibTeX", "\u{E000}BIBTEX\u{E001}");
    result
}

/// Restore protected commands after conversion
pub fn restore_protected_commands(input: &str) -> String {
    let mut result = input.to_string();
    result = result.replace("\u{E000}TODAY\u{E001}", "#datetime.today().display()");
    result = result.replace("\u{E000}LATEX\u{E001}", "LaTeX");
    result = result.replace("\u{E000}TEX\u{E001}", "TeX");
    result = result.replace("\u{E000}XETEX\u{E001}", "XeTeX");
    result = result.replace("\u{E000}LUATEX\u{E001}", "LuaTeX");
    result = result.replace("\u{E000}PDFTEX\u{E001}", "pdfTeX");
    result = result.replace("\u{E000}BIBTEX\u{E001}", "BibTeX");
    result
}

// =============================================================================
// Whitespace Cleaning
// =============================================================================

/// Clean up excessive whitespace in the output.
///
/// This function performs the following normalizations:
/// - Removes leading/trailing blank lines
/// - Collapses multiple consecutive blank lines into one (preserving paragraph breaks)
/// - Trims trailing whitespace on each line
/// - Preserves code blocks (```...```) exactly as-is
pub fn clean_whitespace(input: &str) -> String {
    let mut result = String::new();
    let mut consecutive_newlines = 0;
    let mut in_code_block = false;

    for line in input.lines() {
        let trimmed = line.trim_end();

        // Check for code block delimiters (``` with optional language)
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            // Output code block delimiter as-is
            result.push_str(line);
            result.push('\n');
            consecutive_newlines = 1;
            continue;
        }

        // Inside code block: preserve everything as-is
        if in_code_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Outside code block: apply whitespace cleanup
        if trimmed.is_empty() {
            consecutive_newlines += 1;
            // Allow at most one blank line (which is two newlines in a row)
            if consecutive_newlines <= 2 {
                result.push('\n');
            }
        } else {
            // Non-empty line - reset counter and output
            result.push_str(trimmed);
            result.push('\n');
            consecutive_newlines = 1; // Count this line's newline
        }
    }

    // Remove leading blank lines
    let result = result.trim_start_matches('\n').to_string();
    // Remove trailing blank lines but keep one final newline
    let result = result.trim_end().to_string();
    if result.is_empty() {
        result
    } else {
        result + "\n"
    }
}

// =============================================================================
// AST Text Extraction
// =============================================================================

/// Extract all text from a node (strips braces - use for math/simple content)
pub fn extract_node_text(node: &SyntaxNode) -> String {
    let mut text = String::new();
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Token(t) => {
                if !matches!(
                    t.kind(),
                    SyntaxKind::TokenLBrace
                        | SyntaxKind::TokenRBrace
                        | SyntaxKind::TokenLBracket
                        | SyntaxKind::TokenRBracket
                ) {
                    text.push_str(t.text());
                }
            }
            SyntaxElement::Node(n) => {
                text.push_str(&extract_node_text(&n));
            }
        }
    }
    text
}

/// Extract all text from a node preserving braces (use for text content with commands)
pub fn extract_node_text_with_braces(node: &SyntaxNode) -> String {
    let mut text = String::new();
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Token(t) => {
                text.push_str(t.text());
            }
            SyntaxElement::Node(n) => {
                text.push_str(&extract_node_text_with_braces(&n));
            }
        }
    }
    text
}

/// Extract text content from an argument node
pub fn extract_arg_content(node: &SyntaxNode) -> String {
    let mut content = String::new();
    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::TokenLBrace
            | SyntaxKind::TokenRBrace
            | SyntaxKind::TokenLBracket
            | SyntaxKind::TokenRBracket => continue,
            SyntaxKind::ItemCurly | SyntaxKind::ItemBracket => {
                if let SyntaxElement::Node(n) = child {
                    content.push_str(&extract_node_text(&n));
                }
            }
            _ => {
                if let SyntaxElement::Token(t) = child {
                    content.push_str(t.text());
                } else if let SyntaxElement::Node(n) = child {
                    content.push_str(&extract_node_text(&n));
                }
            }
        }
    }
    content.trim().to_string()
}

/// Extract argument content preserving inner braces but stripping outermost
pub fn extract_arg_content_with_braces(node: &SyntaxNode) -> String {
    let mut content = String::new();
    for child in node.children_with_tokens() {
        match child.kind() {
            // Skip the outermost braces/brackets (direct tokens)
            SyntaxKind::TokenLBrace
            | SyntaxKind::TokenRBrace
            | SyntaxKind::TokenLBracket
            | SyntaxKind::TokenRBracket => continue,
            // For ItemCurly/ItemBracket, extract their *inner* content (skip their braces)
            SyntaxKind::ItemCurly | SyntaxKind::ItemBracket => {
                if let SyntaxElement::Node(n) = child {
                    // Recurse but skip the curly/bracket's own braces
                    content.push_str(&extract_curly_inner_content(&n));
                }
            }
            _ => {
                if let SyntaxElement::Token(t) = child {
                    content.push_str(t.text());
                } else if let SyntaxElement::Node(n) = child {
                    content.push_str(&extract_node_text_with_braces(&n));
                }
            }
        }
    }
    content.trim().to_string()
}

/// Extract inner content of a curly/bracket node, skipping its braces
pub fn extract_curly_inner_content(node: &SyntaxNode) -> String {
    let mut content = String::new();
    for child in node.children_with_tokens() {
        match child.kind() {
            // Skip the braces of this curly node
            SyntaxKind::TokenLBrace
            | SyntaxKind::TokenRBrace
            | SyntaxKind::TokenLBracket
            | SyntaxKind::TokenRBracket => continue,
            _ => {
                if let SyntaxElement::Token(t) = child {
                    content.push_str(t.text());
                } else if let SyntaxElement::Node(n) = child {
                    // For nested structures, preserve their braces
                    content.push_str(&extract_node_text_with_braces(&n));
                }
            }
        }
    }
    content
}

// =============================================================================
// Caption Text Conversion
// =============================================================================

/// Convert caption/title/author text that may contain inline math and formatting commands
/// Handles LaTeX math mode ($...$) and text formatting commands
pub fn convert_caption_text(text: &str) -> String {
    fn read_braced(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
        while let Some(&' ') = chars.peek() {
            chars.next();
        }
        if chars.peek() != Some(&'{') {
            return None;
        }
        chars.next(); // consume '{'
        let mut depth = 1i32;
        let mut content = String::new();
        for ch in chars.by_ref() {
            match ch {
                '{' => {
                    depth += 1;
                    content.push(ch);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    content.push(ch);
                }
                _ => content.push(ch),
            }
        }
        Some(content)
    }

    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Collect math content until closing $, but handle nested $ inside \text{} etc.
            let mut math_content = String::new();
            let mut text_brace_depth = 0i32; // Track braces after \text commands
            let mut in_text_cmd = false;

            while let Some(&next) = chars.peek() {
                if next == '$' && text_brace_depth == 0 {
                    chars.next(); // consume closing $
                    break;
                }

                let c = chars.next().unwrap();
                math_content.push(c);

                // Track \text{} and similar commands to handle nested $
                if c == '\\' {
                    // Check for text commands that may contain nested $
                    let mut cmd = String::new();
                    while let Some(&nc) = chars.peek() {
                        if nc.is_ascii_alphabetic() {
                            cmd.push(chars.next().unwrap());
                            math_content.push(cmd.chars().last().unwrap());
                        } else {
                            break;
                        }
                    }
                    // Commands that can contain nested $ inside their braces
                    if matches!(cmd.as_str(), "text" | "textrm" | "textup" | "textnormal" | "mbox" | "hbox") {
                        in_text_cmd = true;
                    }
                } else if c == '{' && in_text_cmd {
                    text_brace_depth += 1;
                    in_text_cmd = false; // Reset flag after seeing opening brace
                } else if c == '{' && text_brace_depth > 0 {
                    text_brace_depth += 1;
                } else if c == '}' && text_brace_depth > 0 {
                    text_brace_depth -= 1;
                }
            }
            // Convert the math content
            let converted = super::latex_math_to_typst(&math_content);
            let trimmed = converted.trim();
            let prev_non_space = result.chars().rev().find(|c| !c.is_whitespace());
            let attachable = matches!(
                prev_non_space,
                Some(c) if c.is_ascii_alphanumeric() || c == ')' || c == ']' || c == '}'
            );
            let is_sub = trimmed.starts_with('_');
            let is_sup = trimmed.starts_with('^');
            if attachable && (is_sub || is_sup) {
                let mut inner = trimmed[1..].trim();
                if (inner.starts_with('(') && inner.ends_with(')'))
                    || (inner.starts_with('{') && inner.ends_with('}'))
                {
                    inner = &inner[1..inner.len() - 1];
                }
                let inner_trim = inner.trim();
                let content = if inner_trim.starts_with('"') && inner_trim.ends_with('"')
                    && inner_trim.len() >= 2
                {
                    &inner_trim[1..inner_trim.len() - 1]
                } else {
                    inner_trim
                };
                if is_sub {
                    result.push_str("#sub[");
                    result.push_str(content);
                    result.push(']');
                } else {
                    result.push_str("#super[");
                    result.push_str(content);
                    result.push(']');
                }
            } else {
                result.push('$');
                result.push_str(&converted);
                result.push('$');
            }
        } else if ch == '\\' {
            // Handle backslash commands in text mode
            let mut cmd = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_ascii_alphabetic() {
                    cmd.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            // Check for citation-like commands that may take optional arguments
            let is_cite = matches!(
                cmd.as_str(),
                "cite"
                    | "citep"
                    | "citet"
                    | "citealt"
                    | "citeauthor"
                    | "autocite"
                    | "parencite"
                    | "textcite"
            );

            // Capture optional argument for citations (e.g., \cite[p.~47]{key})
            let mut opt_content: Option<String> = None;
            if is_cite {
                while let Some(&' ') = chars.peek() {
                    chars.next();
                }
                if chars.peek() == Some(&'[') {
                    chars.next(); // consume '['
                    let mut content = String::new();
                    let mut depth = 1i32;
                    for c in chars.by_ref() {
                        match c {
                            '[' => {
                                depth += 1;
                                content.push(c);
                            }
                            ']' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                content.push(c);
                            }
                            _ => content.push(c),
                        }
                    }
                    opt_content = Some(content);
                }
            }

            // Check if this command takes a braced argument
            let has_arg = is_cite
                || crate::data::symbols::is_caption_text_command(&cmd)
                || matches!(cmd.as_str(), "href" | "url");

            // Extract argument content if present
            let arg_content = if has_arg {
                // Skip whitespace
                while let Some(&' ') = chars.peek() {
                    chars.next();
                }
                // Check for opening brace
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume '{'
                    let mut content = String::new();
                    let mut brace_depth = 1;
                    for c in chars.by_ref() {
                        if c == '{' {
                            brace_depth += 1;
                            content.push(c);
                        } else if c == '}' {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                            content.push(c);
                        } else {
                            content.push(c);
                        }
                    }
                    Some(content)
                } else {
                    None
                }
            } else {
                None
            };

            // Convert common text-mode commands
            match cmd.as_str() {
                "href" => {
                    let target = arg_content.unwrap_or_default();
                    let label = read_braced(&mut chars).unwrap_or_default();
                    let escaped = escape_typst_text(&target);
                    if !escaped.is_empty() {
                        let rendered = if label.is_empty() {
                            escaped.clone()
                        } else {
                            convert_caption_text(&label)
                        };
                        result.push_str(&format!("#link(\"{}\")[{}]", escaped, rendered));
                    } else if !label.is_empty() {
                        result.push_str(&convert_caption_text(&label));
                    }
                }
                "url" => {
                    if let Some(content) = arg_content {
                        let escaped = escape_typst_text(&content);
                        result.push_str(&format!("#link(\"{0}\")[{0}]", escaped));
                    }
                }
                "textbf" | "bf" => {
                    result.push_str("#strong[");
                    if let Some(content) = arg_content {
                        let text = convert_caption_text(&content);
                        result.push_str(&text);
                        // Prevent trailing backslash from escaping closing bracket
                        if text.ends_with('\\') {
                            result.push(' ');
                        }
                    }
                    result.push(']');
                    result.push(' ');
                }
                "textit" | "it" | "emph" => {
                    result.push_str("#emph[");
                    if let Some(content) = arg_content {
                        let text = convert_caption_text(&content);
                        result.push_str(&text);
                        // Prevent trailing backslash from escaping closing bracket
                        if text.ends_with('\\') {
                            result.push(' ');
                        }
                    }
                    result.push(']');
                    result.push(' ');
                }
                "textsuperscript" => {
                    result.push_str("#super[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                }
                "textsubscript" => {
                    result.push_str("#sub[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                }
                "xspace" => {
                    result.push(' ');
                }
                "texttt" => {
                    result.push('`');
                    if let Some(content) = arg_content {
                        let cleaned = unescape_latex_monospace(&content);
                        result.push_str(&cleaned); // Don't recurse for monospace
                    }
                    result.push('`');
                }
                "code" => {
                    result.push('`');
                    if let Some(content) = arg_content {
                        let cleaned = unescape_latex_monospace(&content);
                        result.push_str(&cleaned); // Don't recurse for monospace
                    }
                    result.push('`');
                }
                "textsc" => {
                    result.push_str("#smallcaps[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                    result.push(' ');
                }
                "pkg" => {
                    result.push_str("#emph[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                    result.push(' ');
                }
                "underline" => {
                    result.push_str("#underline[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                    result.push(' ');
                }
                "textrm" | "text" | "mbox" | "hbox" => {
                    // Just include the content
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                }
                "textsf" => {
                    result.push_str("#text(font: \"sans-serif\")[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                }
                // Date/time commands
                "today" => result.push_str("#datetime.today().display()"),

                // LaTeX logo commands
                "LaTeX" => result.push_str("LaTeX"),
                "TeX" => result.push_str("TeX"),
                "XeTeX" => result.push_str("XeTeX"),
                "LuaTeX" => result.push_str("LuaTeX"),
                "pdfTeX" => result.push_str("pdfTeX"),
                "BibTeX" => result.push_str("BibTeX"),
                "eg" => result.push_str("e.g."),
                "ie" => result.push_str("i.e."),
                "etal" => result.push_str("et al."),
                "vs" => result.push_str("vs."),

                // Common escapes
                "&" => result.push('&'),
                "%" => result.push('%'),
                "_" => result.push_str("\\_"), // _ needs escaping in text mode
                "#" => result.push_str("\\#"), // # needs escaping in Typst
                "$" => result.push_str("\\$"), // $ needs escaping in Typst
                "{" => result.push('{'),
                "}" => result.push('}'),
                "\\" => result.push_str("\\ "), // line break
                "backslash" => result.push('\\'),
                "textbackslash" => result.push('\\'),
                "" => {
                    // Just a backslash followed by non-alpha (like \\ or \&)
                    // Already consumed, do nothing
                }
                "cite" | "citep" | "citet" | "citealt" | "citeauthor" | "autocite"
                | "parencite" | "textcite" => {
                    if let Some(content) = arg_content {
                        let keys: Vec<String> = content
                            .split(',')
                            .map(|k| sanitize_citation_key(k.trim()))
                            .filter(|k| !k.is_empty())
                            .collect();
                        if !keys.is_empty() {
                            result.push('[');
                            for (idx, key) in keys.iter().enumerate() {
                                if idx > 0 {
                                    result.push_str("; ");
                                }
                                result.push('@');
                                result.push_str(key);
                            }
                            result.push(']');
                            if let Some(opt) = opt_content {
                                let opt_clean = convert_caption_text(&opt);
                                if !opt_clean.trim().is_empty() {
                                    result.push(' ');
                                    result.push('(');
                                    result.push_str(opt_clean.trim());
                                    result.push(')');
                                }
                            }
                        }
                    }
                }
                // Reference commands: \ref{label} -> @label
                "ref" | "autoref" | "cref" | "Cref" | "figref" | "tabref" | "secref" => {
                    if let Some(content) = arg_content {
                        let label = sanitize_label(content.trim());
                        if !label.is_empty() {
                            result.push_str("@");
                            result.push_str(&label);
                        }
                    }
                }
                "eqref" => {
                    if let Some(content) = arg_content {
                        let label = sanitize_label(content.trim());
                        if !label.is_empty() {
                            result.push_str("(@");
                            result.push_str(&label);
                            result.push(')');
                        }
                    }
                }
                "pageref" => {
                    if let Some(content) = arg_content {
                        let label = sanitize_label(content.trim());
                        if !label.is_empty() {
                            // Typst doesn't have direct pageref, output as regular ref
                            result.push_str("@");
                            result.push_str(&label);
                        }
                    }
                }
                // Color commands: \textcolor{color}{text} -> #text(fill: color)[text]
                "textcolor" | "color" => {
                    // arg_content has the color, need to read the second argument
                    let text_content = read_braced(&mut chars).unwrap_or_default();
                    let color = arg_content.unwrap_or_default();
                    if !text_content.is_empty() {
                        result.push_str("#text(fill: ");
                        result.push_str(color.trim());
                        result.push_str(")[");
                        result.push_str(&convert_caption_text(&text_content));
                        result.push(']');
                    }
                }
                "colorbox" => {
                    // \colorbox{color}{text} -> #box(fill: color)[text]
                    let text_content = read_braced(&mut chars).unwrap_or_default();
                    let color = arg_content.unwrap_or_default();
                    if !text_content.is_empty() {
                        result.push_str("#box(fill: ");
                        result.push_str(color.trim());
                        result.push_str(")[");
                        result.push_str(&convert_caption_text(&text_content));
                        result.push(']');
                    }
                }
                _ => {
                    // For unknown commands, skip the backslash (don't output raw LaTeX)
                    // If there's an argument, output its content
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    // Otherwise, just skip the unknown command
                }
            }
        } else {
            match ch {
                '@' | '_' | '*' | '#' | '$' | '`' | '<' | '>' => {
                    result.push('\\');
                    result.push(ch);
                }
                _ => result.push(ch),
            }
        }
    }

    result
}

/// Convert author text while preserving \\ and \and separators and dropping footnotes.
pub fn convert_author_text(text: &str) -> String {
    fn read_braced(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
        while let Some(&' ') = chars.peek() {
            chars.next();
        }
        if chars.peek() != Some(&'{') {
            return None;
        }
        chars.next(); // consume '{'
        let mut depth = 1i32;
        let mut content = String::new();
        for ch in chars.by_ref() {
            match ch {
                '{' => {
                    depth += 1;
                    content.push(ch);
                }
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    content.push(ch);
                }
                _ => content.push(ch),
            }
        }
        Some(content)
    }

    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Preserve inline math by converting to Typst math.
            let mut math_content = String::new();
            while let Some(&next) = chars.peek() {
                if next == '$' {
                    chars.next();
                    break;
                }
                math_content.push(chars.next().unwrap());
            }
            let converted = super::latex_math_to_typst(&math_content);
            result.push('$');
            result.push_str(&converted);
            result.push('$');
        } else if ch == '\\' {
            let mut cmd = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_ascii_alphabetic() {
                    cmd.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            match cmd.as_str() {
                "" => {
                    if let Some(&next) = chars.peek() {
                        match next {
                            '\\' => {
                                chars.next();
                                result.push_str("\\\\");
                                if chars.peek() == Some(&'[') {
                                    chars.next();
                                    let mut content = String::new();
                                    while let Some(next) = chars.next() {
                                        if next == ']' {
                                            break;
                                        }
                                        content.push(next);
                                    }
                                    if !content.trim().is_empty() {
                                        result.push('[');
                                        result.push_str(content.trim());
                                        result.push(']');
                                    }
                                } else if matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
                                    let mut content = String::new();
                                    while let Some(&c) = chars.peek() {
                                        if c.is_ascii_whitespace() || c == '\\' || c == ',' {
                                            break;
                                        }
                                        content.push(c);
                                        chars.next();
                                    }
                                    if !content.trim().is_empty() {
                                        result.push('[');
                                        result.push_str(content.trim());
                                        result.push(']');
                                    }
                                }
                            }
                            '&' => {
                                chars.next();
                                result.push('&');
                            }
                            '%' => {
                                chars.next();
                                result.push('%');
                            }
                            '_' => {
                                chars.next();
                                result.push('_');
                            }
                            '#' => {
                                chars.next();
                                result.push('#');
                            }
                            '{' => {
                                chars.next();
                                result.push('{');
                            }
                            '}' => {
                                chars.next();
                                result.push('}');
                            }
                            _ => {}
                        }
                    }
                }
                "and" | "And" | "AND" => {
                    result.push_str("\\and");
                }
                "thanks" | "footnote" | "footnotemark" | "footnotetext" => {
                    let _ = read_braced(&mut chars);
                }
                "textbf" | "bf" => {
                    result.push_str("#strong[");
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                    result.push(']');
                }
                "normalfont" => {
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                }
                "vspace" => {
                    let _ = read_braced(&mut chars);
                }
                "hspace" => {
                    let _ = read_braced(&mut chars);
                }
                "linkbutton" => {
                    let _ = read_braced(&mut chars);
                    let _ = read_braced(&mut chars);
                    let _ = read_braced(&mut chars);
                }
                "coloremojicode" => {
                    if let Some(content) = read_braced(&mut chars) {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            result.push_str("emoji-");
                            result.push_str(trimmed);
                        }
                    }
                }
                "textit" | "it" | "emph" | "itshape" => {
                    result.push_str("#emph[");
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                    result.push(']');
                }
                "textsuperscript" => {
                    result.push_str("#super[");
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                    result.push(']');
                }
                "textsubscript" => {
                    result.push_str("#sub[");
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                    result.push(']');
                }
                "xspace" => {
                    result.push(' ');
                }
                "texttt" => {
                    result.push('`');
                    if let Some(content) = read_braced(&mut chars) {
                        // Unescape common LaTeX specials for monospace content
                        let unescaped = content
                            .replace("\\@", "@")
                            .replace("\\_", "_")
                            .replace("\\#", "#")
                            .replace("\\%", "%")
                            .replace("\\&", "&")
                            .replace("\\$", "$");
                        result.push_str(&unescaped);
                    }
                    result.push('`');
                }
                "textsc" => {
                    result.push_str("#smallcaps[");
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                    result.push(']');
                }
                "underline" => {
                    result.push_str("#underline[");
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                    result.push(']');
                }
                "textrm" | "text" | "mbox" | "hbox" | "textsf" => {
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                }
                "url" => {
                    if let Some(content) = read_braced(&mut chars) {
                        let escaped = escape_typst_text(&content);
                        result.push_str(&format!("#link(\"{0}\")[{0}]", escaped));
                    }
                }
                "href" => {
                    let target = read_braced(&mut chars).unwrap_or_default();
                    if let Some(content) = read_braced(&mut chars) {
                        let escaped = escape_typst_text(&target);
                        let label = convert_author_text(&content);
                        if !escaped.is_empty() {
                            result.push_str(&format!("#link(\"{}\")[{}]", escaped, label));
                        } else {
                            result.push_str(&label);
                        }
                    }
                }
                "LaTeX" => result.push_str("LaTeX"),
                "TeX" => result.push_str("TeX"),
                "XeTeX" => result.push_str("XeTeX"),
                "LuaTeX" => result.push_str("LuaTeX"),
                "pdfTeX" => result.push_str("pdfTeX"),
                "BibTeX" => result.push_str("BibTeX"),
                "eg" => result.push_str("e.g."),
                "ie" => result.push_str("i.e."),
                "etal" => result.push_str("et al."),
                // JMLR author macros: \name, \email, \addr consume text to next macro/newline
                // Note: Parser may combine e.g. "\email one@" into "\emailone@" so we handle that
                _ if cmd.starts_with("name") => {
                    // Extract any text that got merged with the command name
                    let suffix = &cmd[4..]; // text after "name"
                    let mut content = suffix.to_string();
                    // Skip leading whitespace
                    while let Some(&' ') = chars.peek() {
                        chars.next();
                    }
                    // Read text until next backslash command or newline
                    while let Some(&ch) = chars.peek() {
                        if ch == '\\' || ch == '\n' {
                            break;
                        }
                        content.push(chars.next().unwrap());
                    }
                    result.push_str(content.trim());
                }
                _ if cmd.starts_with("email") => {
                    // Extract any text that got merged with the command name
                    let suffix = &cmd[5..]; // text after "email"
                    let mut content = suffix.to_string();
                    // Skip leading whitespace
                    while let Some(&' ') = chars.peek() {
                        chars.next();
                    }
                    // Read text until next backslash command or newline
                    while let Some(&ch) = chars.peek() {
                        if ch == '\\' || ch == '\n' {
                            break;
                        }
                        content.push(chars.next().unwrap());
                    }
                    // Output email on a new line so it's parsed separately
                    result.push_str("\n");
                    result.push_str(content.trim());
                }
                _ if cmd.starts_with("addr") => {
                    // Extract any text that got merged with the command name
                    let suffix = &cmd[4..]; // text after "addr"
                    let mut content = suffix.to_string();
                    // Skip leading whitespace
                    while let Some(&' ') = chars.peek() {
                        chars.next();
                    }
                    // Read text until next backslash command or newline
                    while let Some(&ch) = chars.peek() {
                        if ch == '\\' || ch == '\n' {
                            break;
                        }
                        content.push(chars.next().unwrap());
                    }
                    // Output on new line
                    result.push_str("\n");
                    result.push_str(content.trim());
                }
                _ => {
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                }
            }
        } else if ch == '%' {
            // Skip LaTeX comments inside author blocks.
            while let Some(next) = chars.next() {
                if next == '\n' {
                    break;
                }
            }
        } else if ch == '~' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{
        convert_author_text, normalize_display_dollars, normalize_math_delimiters,
        normalize_unmatched_braces, replace_verb_commands, strip_label_from_text,
    };

    #[test]
    fn author_text_drops_thanks_and_comments() {
        let input = "John Doe\\thanks{note} \\\\\nDept\\And Jane % comment\n";
        let out = convert_author_text(input);
        assert!(!out.contains("note"));
        assert!(!out.contains('%'));
        assert!(out.contains("\\\\"));
        assert!(out.contains("\\and"));
    }

    #[test]
    fn author_text_preserves_formatting() {
        let input = "\\textbf{Greg} (\\texttt{a@b})\\\\[4pt]\\textit{Team}";
        let out = convert_author_text(input);
        assert!(out.contains("#strong[Greg]"));
        assert!(out.contains("`a@b`"));
        assert!(out.contains("\\\\[4pt]"));
        assert!(out.contains("#emph[Team]"));
    }

    #[test]
    fn author_text_unescapes_texttt_specials() {
        let input = "\\textbf{Greg} (\\texttt{a\\@b\\_c})";
        let out = convert_author_text(input);
        assert!(out.contains("`a@b_c`"));
    }

    #[test]
    fn unicode_preserved_in_preprocess_helpers() {
        let input = "Cafe\u{2014}test \\\\label{sec:intro} $x$";
        let out = normalize_math_delimiters(input);
        let out = normalize_display_dollars(&out);
        let out = normalize_unmatched_braces(&out);
        let (stripped, label) = strip_label_from_text(&out);
        assert_eq!(label.as_deref(), Some("sec:intro"));
        assert!(stripped.contains('\u{2014}'));
    }

    #[test]
    fn replace_verb_keeps_unicode() {
        let input = "code \\\\verb|a{b}\u{2014}c| end";
        let out = replace_verb_commands(input);
        assert!(out.contains("\\texttt{a\\{b\\}—c}"));
    }
}
