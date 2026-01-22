//! Math formula handling for LaTeX to Typst conversion
//!
//! This module handles math formulas, delimiters, and math-specific constructs.

use mitex_parser::syntax::{FormulaItem, SyntaxElement, SyntaxKind};
use rowan::ast::AstNode;
use std::fmt::Write;

use super::context::{ConversionMode, LatexConverter};

/// Convert a math formula ($..$ or $$..$$)
pub fn convert_formula(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    if let SyntaxElement::Node(n) = elem {
        if let Some(formula) = FormulaItem::cast(n.clone()) {
            let is_inline = formula.is_inline();
            let prev_mode = conv.state.mode;
            conv.state.mode = ConversionMode::Math;

            // Collect math content into a buffer for post-processing
            let mut math_content = String::new();
            conv.visit_node(&n, &mut math_content);

            // Apply math cleanup and strip stray delimiters if the parser left them behind
            let cleaned = conv.cleanup_math_spacing(&math_content);
            let cleaned = super::utils::strip_unescaped_dollars(&cleaned);
            let mut cleaned = cleaned.trim().to_string();
            if cleaned.starts_with('/') {
                cleaned = format!("\"{}\"", cleaned.trim());
            }
            loop {
                let bytes = cleaned.as_bytes();
                if cleaned.len() >= 2 && bytes[0] == b'$' && bytes[cleaned.len() - 1] == b'$' {
                    cleaned = cleaned[1..cleaned.len() - 1].trim().to_string();
                } else {
                    break;
                }
            }

            if is_inline {
                output.push('$');
                output.push_str(&cleaned);
                output.push('$');
            } else {
                output.push_str("$ ");
                output.push_str(&cleaned);
                output.push_str(" $");
            }

            conv.state.mode = prev_mode;
        }
    }
}

/// Convert a curly group in math mode
pub fn convert_curly(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    if conv.state.in_preamble {
        return;
    }

    let node = match elem {
        SyntaxElement::Node(n) => n,
        _ => return,
    };

    // Drop empty math placeholders like {$ $} in text mode.
    if matches!(conv.state.mode, ConversionMode::Text) {
        let raw = node.text().to_string();
        let inner = raw.trim().trim_start_matches('{').trim_end_matches('}');
        if !inner.is_empty() && inner.chars().all(|c| c == '$' || c.is_whitespace()) {
            return;
        }
    }

    // Check if this is an argument for a pending operator (operatorname*)
    if let Some(op) = conv.state.pending_op.take() {
        // This group is the argument for a pending operator
        let mut content = String::new();
        // Extract content without braces
        for child in node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::TokenWhiteSpace
                | SyntaxKind::TokenLineBreak
                | SyntaxKind::TokenLBrace
                | SyntaxKind::TokenRBrace => {}
                _ => conv.visit_element(child, &mut content),
            }
        }
        let text = content.trim();

        // Handle common operator patterns that might include spacing commands
        // e.g. "arg thin min" -> "argmin"
        let normalized = text.replace("thin", "").replace(" ", "");
        let final_text = if normalized == "argmin" {
            "argmin"
        } else if normalized == "argmax" {
            "argmax"
        } else {
            text
        };

        // Try to keep it as simple text if possible for cleaner output
        let op_content = if final_text
            .chars()
            .all(|c| c.is_alphanumeric() || c.is_whitespace())
        {
            format!("\"{}\"", final_text)
        } else {
            // Wrap in content block if complex
            format!("[{}]", final_text)
        };

        if op.is_limits {
            let _ = write!(output, "limits(op({}))", op_content);
        } else {
            let _ = write!(output, "op({})", op_content);
        }
        return;
    }

    // Check if it's empty
    let mut has_content = false;
    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::TokenWhiteSpace
            | SyntaxKind::TokenLineBreak
            | SyntaxKind::TokenLBrace
            | SyntaxKind::TokenRBrace => {}
            _ => has_content = true,
        }
        conv.visit_element(child, output);
    }
    // Add zero-width space for empty groups in math mode
    if !has_content && matches!(conv.state.mode, ConversionMode::Math) {
        output.push_str("zws ");
    }
}

/// Convert \left...\right with enhanced delimiter handling
/// Based on tex2typst's comprehensive approach
pub fn convert_lr(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    let node = match elem {
        SyntaxElement::Node(n) => n,
        _ => return,
    };

    let children: Vec<_> = node.children_with_tokens().collect();

    // Extract left and right delimiters
    let mut left_delim: Option<String> = None;
    let mut right_delim: Option<String> = None;
    let mut body_start = 0;
    let mut body_end = children.len();

    // Parse the \left delimiter - it can be a ClauseLR node or a Token
    // First pass: find left delimiter
    for (i, child) in children.iter().enumerate() {
        match child {
            // ClauseLR node contains the delimiter
            SyntaxElement::Node(cn) if cn.kind() == SyntaxKind::ClauseLR => {
                let text = cn.text().to_string();
                if text.starts_with("\\left") && left_delim.is_none() {
                    // Extract delimiter from inside the ClauseLR
                    // The delimiter can be a Token (like "(") or a Node (like \lVert command)
                    // First try to find it in the full text after \left
                    if let Some(delim_text) = text.strip_prefix("\\left") {
                        let delim_text = delim_text.trim();
                        if !delim_text.is_empty() {
                            // Extract just the delimiter part (first command or symbol)
                            let delim = extract_delimiter_from_text(delim_text);
                            left_delim = Some(convert_delimiter(delim));
                        }
                    }
                    body_start = i + 1;
                }
            }
            // Legacy: Token-based parsing
            SyntaxElement::Token(t) => {
                let name = t.text();
                if let Some(stripped) = name.strip_prefix("\\left") {
                    left_delim = Some(convert_delimiter(stripped));
                    body_start = i + 1;
                }
            }
            _ => {}
        }
    }

    // Second pass: find right delimiter (from the end)
    for (i, child) in children.iter().enumerate().rev() {
        match child {
            SyntaxElement::Node(cn) if cn.kind() == SyntaxKind::ClauseLR => {
                let text = cn.text().to_string();
                if text.starts_with("\\right") && right_delim.is_none() {
                    // Extract delimiter from inside the ClauseLR
                    // First try to find it in the full text after \right
                    if let Some(delim_text) = text.strip_prefix("\\right") {
                        let delim_text = delim_text.trim();
                        if !delim_text.is_empty() {
                            // Extract just the delimiter part (first command or symbol)
                            let delim = extract_delimiter_from_text(delim_text);
                            right_delim = Some(convert_delimiter(delim));
                        }
                    }
                    body_end = i;
                    break;
                }
            }
            SyntaxElement::Token(t) => {
                let name = t.text();
                if let Some(stripped) = name.strip_prefix("\\right") {
                    right_delim = Some(convert_delimiter(stripped));
                    body_end = i;
                    break;
                }
            }
            _ => {}
        }
    }

    // Check for common optimizations (matching pairs that don't need lr())
    // Also handle mismatched or missing delimiters gracefully
    let (use_lr, is_valid_pair) = match (left_delim.as_deref(), right_delim.as_deref()) {
        // These pairs work naturally in Typst without lr()
        (Some("("), Some(")")) | (Some("["), Some("]")) | (Some("{"), Some("}")) => (false, true),
        // Matching pairs that need lr()
        (Some(l), Some(r)) if l == r => (true, true),
        // Valid mixed pairs that lr() can handle
        (Some("("), Some("]"))
        | (Some("["), Some(")"))
        | (Some("chevron.l"), Some("chevron.r"))
        | (Some("floor.l"), Some("floor.r"))
        | (Some("ceil.l"), Some("ceil.r")) => (true, true),
        // Empty delimiter on one side - valid for lr()
        (Some("."), Some(_)) | (Some(_), Some(".")) => (true, true),
        // Missing delimiter - don't use lr(), just output content
        (None, _) | (_, None) => (false, false),
        // Other cases - try lr() but mark as potentially invalid
        _ => (true, true),
    };

    // Check for norm: \left\| ... \right\| -> norm(...)
    if left_delim.as_deref() == Some("bar.v.double")
        && right_delim.as_deref() == Some("bar.v.double")
    {
        // Collect content first to check for commas
        let mut content = String::new();
        for child in children.iter().take(body_end).skip(body_start) {
            match child {
                SyntaxElement::Token(t) if t.text() == "." => {}
                SyntaxElement::Token(t) if t.text().starts_with("\\right") => {}
                _ => conv.visit_element(child.clone(), &mut content),
            }
        }
        // Wrap in {} if content contains comma to prevent parsing as function args
        output.push_str("norm(");
        if content.contains(',') {
            output.push('{');
            output.push_str(content.trim());
            output.push('}');
        } else {
            output.push_str(&content);
        }
        output.push_str(") ");
        return;
    }

    // Check for abs: \left| ... \right| -> abs(...)
    if left_delim.as_deref() == Some("bar.v") && right_delim.as_deref() == Some("bar.v") {
        // Collect content first to check for commas
        let mut content = String::new();
        for child in children.iter().take(body_end).skip(body_start) {
            match child {
                SyntaxElement::Token(t) if t.text() == "." => {}
                SyntaxElement::Token(t) if t.text().starts_with("\\right") => {}
                _ => conv.visit_element(child.clone(), &mut content),
            }
        }
        // Wrap in {} if content contains comma to prevent parsing as function args
        output.push_str("abs(");
        if content.contains(',') {
            output.push('{');
            output.push_str(content.trim());
            output.push('}');
        } else {
            output.push_str(&content);
        }
        output.push_str(") ");
        return;
    }

    // For invalid pairs (missing delimiters), just output the content without lr()
    if !is_valid_pair {
        // Output left delimiter if present
        if let Some(ref delim) = left_delim {
            if delim != "." && !delim.is_empty() {
                output.push_str(delim);
                output.push(' ');
            }
        }

        // Output body content
        for child in children.iter().take(body_end).skip(body_start) {
            match child {
                SyntaxElement::Token(t) if t.text() == "." => {}
                SyntaxElement::Token(t) if t.text().starts_with("\\right") => {}
                _ => conv.visit_element(child.clone(), output),
            }
        }

        // Output right delimiter if present
        if let Some(ref delim) = right_delim {
            if delim != "." && !delim.is_empty() {
                output.push_str(delim);
                output.push(' ');
            }
        }
        return;
    }

    // Output with or without lr()
    if use_lr {
        output.push_str("lr(");
    }

    // Output left delimiter
    if let Some(ref delim) = left_delim {
        if delim != "." && !delim.is_empty() {
            output.push_str(delim);
            output.push(' ');
        }
    }

    // Output body content
    for child in children.iter().take(body_end).skip(body_start) {
        match child {
            SyntaxElement::Token(t) if t.text() == "." => {}
            SyntaxElement::Token(t) if t.text().starts_with("\\right") => {}
            _ => conv.visit_element(child.clone(), output),
        }
    }

    // Output right delimiter with space before for clarity
    if let Some(ref delim) = right_delim {
        if delim != "." && !delim.is_empty() {
            output.push(' ');
            output.push_str(delim);
        }
    }

    if use_lr {
        output.push_str(") ");
    } else {
        output.push(' ');
    }
}

/// Convert subscript/superscript attachment
pub fn convert_attachment(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    let node = match elem {
        SyntaxElement::Node(n) => n,
        _ => return,
    };

    let mut is_script = false;

    for child in node.children_with_tokens() {
        let kind = child.kind();

        if kind == SyntaxKind::TokenUnderscore {
            output.push('_');
            is_script = true;
            continue;
        }

        if kind == SyntaxKind::TokenCaret {
            output.push('^');
            is_script = true;
            continue;
        }

        // Skip whitespace
        if kind == SyntaxKind::TokenWhiteSpace || kind == SyntaxKind::TokenLineBreak {
            // Check if previous char is _ or ^, if so, don't output space yet
            // Wait until after the script content
            continue;
        }

        if is_script {
            // Always wrap attachment content in parentheses to ensure correct binding
            // e.g. sum_i=1 -> sum_(i=1) instead of sum_i = 1
            output.push('(');
            conv.visit_element(child, output);
            output.push(')');
            // No space after script to ensure tight binding of multiple scripts
            is_script = false;
        } else {
            // This handles the base or other parts if any (though usually base is previous sibling)
            conv.visit_element(child, output);
        }
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Extract delimiter from text after \left or \right.
///
/// This function handles all LaTeX delimiter forms:
/// - Single character: `(`, `)`, `[`, `]`, `|`, `.`
/// - Letter-based commands: `\langle`, `\rangle`, `\lVert`, `\lfloor`, etc.
/// - Non-letter commands: `\|`, `\{`, `\}`
fn extract_delimiter_from_text(text: &str) -> &str {
    if text.is_empty() {
        return ".";
    }

    if let Some(after_backslash) = text.strip_prefix('\\') {
        // Check if the character after \ is a letter
        if after_backslash.is_empty() {
            return "\\";
        }

        let first_char = after_backslash.chars().next().unwrap();
        if first_char.is_ascii_alphabetic() {
            // Letter-based command: \langle, \lVert, etc.
            // Find where the command name ends
            let end = after_backslash
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(after_backslash.len());
            &text[..end + 1] // +1 for the backslash
        } else {
            // Non-letter command: \|, \{, \}
            // The command is exactly \ + one character
            let char_len = first_char.len_utf8();
            &text[..1 + char_len]
        }
    } else {
        // Single character delimiter: (, ), [, ], |, .
        let first_char = text.chars().next().unwrap();
        &text[..first_char.len_utf8()]
    }
}

/// Convert a LaTeX delimiter to Typst equivalent
fn convert_delimiter(delim: &str) -> String {
    match delim.trim() {
        "." => ".".to_string(), // Empty delimiter
        "(" => "(".to_string(),
        ")" => ")".to_string(),
        "[" => "[".to_string(),
        "]" => "]".to_string(),
        "\\{" | "\\lbrace" => "{".to_string(),
        "\\}" | "\\rbrace" => "}".to_string(),
        "|" | "\\vert" | "\\lvert" | "\\rvert" => "bar.v".to_string(),
        "\\|" | "\\Vert" | "\\lVert" | "\\rVert" => "bar.v.double".to_string(),
        "\\langle" => "chevron.l".to_string(),
        "\\rangle" => "chevron.r".to_string(),
        "\\lfloor" => "floor.l".to_string(),
        "\\rfloor" => "floor.r".to_string(),
        "\\lceil" => "ceil.l".to_string(),
        "\\rceil" => "ceil.r".to_string(),
        "\\lgroup" => "paren.l.flat".to_string(),
        "\\rgroup" => "paren.r.flat".to_string(),
        other => other.to_string(),
    }
}
