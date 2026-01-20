use typst_syntax::{parse, SyntaxKind, SyntaxNode};
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, is_two_column,
    parse_length_to_pt, render_amsthm_definitions,
};

#[derive(Debug, Default)]
struct LetterMeta {
    sender: Option<String>,
    recipient: Option<String>,
    date: Option<String>,
    subject: Option<String>,
    name: Option<String>,
}

pub fn maybe_convert_letter(input: &str) -> Option<String> {
    let root = parse(input);
    let show = find_show_with(&root, "letter.with")?;
    let meta = extract_meta(&show);

    let doc = typst_to_ir(input);
    let hints = extract_preamble_hints(input);
    let base_font_size_pt =
        hints.text_size.as_deref().and_then(|size| parse_length_to_pt(size, "10pt"));
    let cite_command = hints
        .cite_command
        .clone()
        .or_else(|| if hints.uses_natbib { Some("citep".to_string()) } else { None });
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: is_two_column(&hints),
            inline_wide_tables: false,
            force_here: true,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Bottom,
            bibliography_style_default: hints.bibliography_style.clone(),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    );

    let mut out = String::new();
    out.push_str("\\documentclass{letter}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{float}\n");
    out.push_str("\\usepackage{hyperref}\n");
    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if hints.uses_natbib {
        out.push_str("\\usepackage{natbib}\n");
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }
    out.push_str("\\begin{document}\n");
    if let Some(sender) = meta.sender.as_deref() {
        out.push_str("\\address{");
        out.push_str(&escape_latex(sender));
        out.push_str("}\n");
    }
    if let Some(name) = meta.name.as_deref() {
        out.push_str("\\signature{");
        out.push_str(&escape_latex(name));
        out.push_str("}\n");
    }
    out.push_str("\\begin{letter}{");
    if let Some(recipient) = meta.recipient.as_deref() {
        out.push_str(&escape_latex(recipient));
    }
    out.push_str("}\n");

    if let Some(subject) = meta.subject.as_deref() {
        out.push_str("\\textbf{Subject: ");
        out.push_str(&escape_latex(subject));
        out.push_str("}\\\\\n");
    }
    if let Some(date) = meta.date.as_deref() {
        out.push_str("\\textbf{Date: ");
        out.push_str(&escape_latex(date));
        out.push_str("}\\\\\n");
    }

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }

    out.push_str("\\end{letter}\n");
    out.push_str("\\end{document}\n");
    Some(out)
}

fn find_show_with(root: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::ShowRule {
            if let Some(func) = node.children().find(|c| c.kind() == SyntaxKind::FuncCall) {
                if let Some(func_name) = func_call_name(&func) {
                    if func_name == name {
                        return Some(node);
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

fn extract_meta(show_rule: &SyntaxNode) -> LetterMeta {
    let mut meta = LetterMeta::default();
    let Some(func) = show_rule.children().find(|c| c.kind() == SyntaxKind::FuncCall) else {
        return meta;
    };
    let Some(args) = func.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return meta;
    };

    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        match key.as_str() {
            "sender" => meta.sender = extract_string_like(&value),
            "recipient" => meta.recipient = extract_string_like(&value),
            "date" => meta.date = extract_string_like(&value),
            "subject" => meta.subject = extract_string_like(&value),
            "name" => meta.name = extract_string_like(&value),
            _ => {}
        }
    }
    meta
}

fn extract_string_like(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Text => Some(node.text().to_string()),
        SyntaxKind::Ident => Some(node.text().to_string()),
        SyntaxKind::ContentBlock | SyntaxKind::Markup => Some(extract_markup_text(node)),
        SyntaxKind::Array => {
            let values = extract_array_strings(node);
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        }
        _ => Some(node_full_text(node)),
    }
}

fn extract_array_strings(node: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::LeftParen
            | SyntaxKind::RightParen
            | SyntaxKind::Comma
            | SyntaxKind::Space => continue,
            _ => {}
        }
        if let Some(value) = extract_string_like(&child) {
            out.push(value);
        }
    }
    out
}

fn extract_markup_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Text | SyntaxKind::Str => out.push_str(child.text().trim_matches('"')),
            SyntaxKind::Space => out.push(' '),
            _ => out.push_str(&extract_markup_text(&child)),
        }
    }
    out
}

fn extract_named_key(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|c| c.kind() == SyntaxKind::Ident)
        .map(|n| n.text().to_string())
}

fn extract_named_value_node(node: &SyntaxNode) -> Option<SyntaxNode> {
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

fn func_call_name(node: &SyntaxNode) -> Option<String> {
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

fn node_full_text(node: &SyntaxNode) -> String {
    node.clone().into_text().to_string()
}

fn escape_latex(input: &str) -> String {
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
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            _ => out.push(ch),
        }
    }
    out
}
