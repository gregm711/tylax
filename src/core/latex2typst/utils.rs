//! Utility functions for LaTeX to Typst conversion
//!
//! This module contains pure utility functions that don't depend on converter state.

use mitex_parser::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use std::collections::HashSet;
use std::fmt::Write;

// =============================================================================
// Text Processing Utilities
// =============================================================================

/// Normalize table cell text by cleaning up whitespace issues
/// This handles cases where MiTeX tokenizer may have added extra spaces between characters
pub fn normalize_cell_text(text: &str) -> String {
    // If the text contains special Typst cell markers, leave it as is
    if text.starts_with("___TYPST_CELL___:") {
        return text.to_string();
    }

    let mut result = String::new();
    let mut chars = text.chars().peekable();
    let mut last_was_space = false;

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            // Check if this might be spurious space between word characters
            // Pattern: "letter space letter" with single char before space suggests bad tokenization
            if !result.is_empty() && !last_was_space {
                // Look ahead to see if next non-space is a letter
                let next_non_space = chars.clone().find(|c| !c.is_whitespace());

                // Check if last char in result is part of a word and next char continues it
                let last_char = result.chars().last();
                if let (Some(last), Some(next)) = (last_char, next_non_space) {
                    // If both are alphanumeric characters and result ends with a single character after space,
                    // this might indicate spurious tokenization (e.g. "T e X").
                    // We only collapse single-char spaces between alphanumeric characters to preserve intentional spacing.
                    if ch == ' ' && last.is_alphanumeric() && next.is_alphanumeric() {
                        // Check if this looks like broken-up text (single chars separated by spaces)
                        // by looking at context - if result has "X " pattern repeatedly, collapse
                        let result_chars: Vec<char> = result.chars().collect();
                        if result_chars.len() >= 2 {
                            let prev_prev = result_chars.get(result_chars.len() - 2);
                            // If pattern is: "char space char space" - likely broken tokenization
                            if prev_prev == Some(&' ') {
                                // Skip this space to collapse
                                last_was_space = true;
                                continue;
                            }
                        }
                    }
                }
                result.push(' ');
                last_was_space = true;
            } else if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }

    result.trim().to_string()
}

/// Sanitize a label name for Typst compatibility
/// Converts colons to hyphens since Typst labels work better with hyphens
pub fn sanitize_label(label: &str) -> String {
    label.replace([':', ' ', '_'], "-")
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
            if cmd == "\\bibliography" && after < bytes.len() && bytes[after].is_ascii_alphabetic() {
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
    sanitize_bibtex_keys(&converted)
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
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if i + 1 < bytes.len() {
                out.push(bytes[i] as char);
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'$' {
            if i > 0 && bytes[i - 1] == b'\\' {
                out.push('$');
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
                    out.push_str("\\textsuperscript{");
                    out.push_str(content);
                    out.push('}');
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Avoid accidental "*/" sequences before loss markers (e.g. "*/* tylax:loss:" -> "* /* tylax:loss:")
pub fn sanitize_loss_comment_boundaries(input: &str) -> String {
    input.replace("*/* tylax:loss:", "* /* tylax:loss:")
}

// =============================================================================
// Math Cleanup Helpers
// =============================================================================

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
    let before = func[..func_start].chars().rev().find(|c| !c.is_whitespace());
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
    let mut out = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
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
                } else {
                    out.push_str(&cmd);
                }
            }
            '$' | '{' | '}' => {}
            _ => out.push(ch),
        }
    }

    out.trim().to_string()
}

fn convert_string_entries(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let tail = &input[i..];
            if tail.len() >= 8 && tail[..8].eq_ignore_ascii_case("@string(") {
                out.push_str("@string{");
                let mut j = i + 8;
                let mut depth = 1usize;
                while j < bytes.len() {
                    match bytes[j] as char {
                        '(' => depth += 1,
                        ')' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                out.push_str(&input[i + 8..j]);
                                out.push('}');
                                i = j + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if j >= bytes.len() {
                    out.push_str(&input[i + 8..]);
                    break;
                }
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn sanitize_bibtex_keys(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'@' {
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
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let key_start = j;
                while j < bytes.len() && bytes[j] != b',' && bytes[j] != b'\n' && bytes[j] != b'\r' {
                    j += 1;
                }
                let key_raw = input[key_start..j].trim();
                let mut k = j;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b',' {
                    let sanitized = sanitize_citation_key(key_raw);
                    out.push('@');
                    out.push_str(&entry_type);
                    out.push(open);
                    out.push_str(&sanitized);
                    out.push(',');
                    i = k + 1;
                    continue;
                }
            }
            out.push_str(&input[start..i]);
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Escape plain text for Typst markup.
/// This is applied to non-math text tokens to avoid accidental markup (e.g., emails, underscores).
pub fn escape_typst_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '@' | '_' | '*' | '#' | '$' | '`' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
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
    let mut out = String::with_capacity(raw.len());
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
        out.push(bytes[i] as char);
        i += 1;
    }
    let cleaned = out.trim().to_string();
    let cleaned_label = label.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    (cleaned, cleaned_label)
}

/// Escape '@' occurrences that are not valid Typst references or citations.
pub fn escape_at_in_words(input: &str) -> String {
    let labels = collect_emitted_labels(input);
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0usize;

    while i < len {
        let ch = chars[i];
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

            if !prev_is_escape && !prev_is_cite && !candidate.is_empty() && !labels.contains(&candidate) {
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
    let mut out = input.replace("\\`\\`", "\"");
    out = out.replace("``", "\"");
    out = out.replace("''", "\"");
    out
}

/// Replace \verb delimiters with a brace-based \texttt{...} form so the parser can handle it.
pub fn replace_verb_commands(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if input[i..].starts_with("\\verb") {
                let mut j = i + 5;
                if j < bytes.len() && bytes[j] == b'*' {
                    j += 1;
                }
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= bytes.len() {
                    out.push_str(&input[i..]);
                    break;
                }
                let delim = bytes[j] as char;
                j += 1;
                let start = j;
                while j < bytes.len() && bytes[j] as char != delim {
                    j += 1;
                }
                if j >= bytes.len() {
                    out.push_str(&input[i..]);
                    break;
                }
                let content = &input[start..j];
                let mut escaped = String::with_capacity(content.len());
                for ch in content.chars() {
                    match ch {
                        '{' => escaped.push_str("\\{"),
                        '}' => escaped.push_str("\\}"),
                        _ => escaped.push(ch),
                    }
                }
                out.push_str("\\texttt{");
                out.push_str(&escaped);
                out.push('}');
                i = j + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
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
    let mut out = String::with_capacity(input.len());
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
                    out.push(bytes[i] as char);
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
                            out.push_str(&input[i..end_idx]);
                            i = end_idx;
                            continue;
                        }
                    }

                    if let Ok(content) = std::fs::read_to_string(&full_path) {
                        seen.insert(full_path.clone());
                        let next_base = full_path.parent().unwrap_or(base_dir);
                        let expanded = expand_latex_inputs_inner(&content, next_base, depth + 1, seen);
                        out.push_str(&expanded);
                        if end_idx > 0 {
                            i = end_idx;
                        } else {
                            i = j;
                        }
                        continue;
                    } else if end_idx > i {
                        out.push_str(&input[i..end_idx]);
                        i = end_idx;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
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
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Collect math content until closing $
            let mut math_content = String::new();
            while let Some(&next) = chars.peek() {
                if next == '$' {
                    chars.next(); // consume closing $
                    break;
                }
                math_content.push(chars.next().unwrap());
            }
            // Convert the math content
            let converted = super::latex_math_to_typst(&math_content);
            result.push('$');
            result.push_str(&converted);
            result.push('$');
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

            // Check if this command takes a braced argument
            let has_arg = crate::data::symbols::is_caption_text_command(&cmd);

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
                "textbf" | "bf" => {
                    result.push('*');
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push('*');
                }
                "textit" | "it" | "emph" => {
                    result.push('_');
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push('_');
                }
                "texttt" => {
                    result.push('`');
                    if let Some(content) = arg_content {
                        result.push_str(&content); // Don't recurse for monospace
                    }
                    result.push('`');
                }
                "textsc" => {
                    result.push_str("#smallcaps[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
                }
                "underline" => {
                    result.push_str("#underline[");
                    if let Some(content) = arg_content {
                        result.push_str(&convert_caption_text(&content));
                    }
                    result.push(']');
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

                // Common escapes
                "&" => result.push('&'),
                "%" => result.push('%'),
                "_" => result.push_str("\\_"), // _ needs escaping in text mode
                "#" => result.push_str("\\#"), // # needs escaping in Typst
                "$" => result.push_str("\\$"), // $ needs escaping in Typst
                "{" => result.push('{'),
                "}" => result.push('}'),
                "\\" => result.push_str("\\ "), // line break
                "" => {
                    // Just a backslash followed by non-alpha (like \\ or \&)
                    // Already consumed, do nothing
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
            result.push(ch);
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
                "texttt" | "textbf" | "textit" | "emph" | "textsc" | "underline" | "textrm"
                | "text" | "mbox" | "hbox" | "textsf" => {
                    if let Some(content) = read_braced(&mut chars) {
                        result.push_str(&convert_author_text(&content));
                    }
                }
                "LaTeX" => result.push_str("LaTeX"),
                "TeX" => result.push_str("TeX"),
                "XeTeX" => result.push_str("XeTeX"),
                "LuaTeX" => result.push_str("LuaTeX"),
                "pdfTeX" => result.push_str("pdfTeX"),
                "BibTeX" => result.push_str("BibTeX"),
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
    use super::convert_author_text;

    #[test]
    fn author_text_drops_thanks_and_comments() {
        let input = "John Doe\\thanks{note} \\\\\nDept\\And Jane % comment\n";
        let out = convert_author_text(input);
        assert!(!out.contains("note"));
        assert!(!out.contains('%'));
        assert!(out.contains("\\\\"));
        assert!(out.contains("\\and"));
    }
}
