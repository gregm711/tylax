use typst_syntax::parse;
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, parse_length_to_pt,
    render_amsthm_definitions,
};
use crate::template_adapters::common::{
    escape_latex, extract_named_args, extract_option_bool, extract_string_like,
    extract_year_from_name, find_show_rule_with_prefix, resolve_ident, extract_array_elements,
    extract_array_strings,
};

#[derive(Debug, Clone)]
struct IclrAuthorGroup {
    names: Vec<String>,
    affiliation: Option<String>,
    address: Option<String>,
    email: Option<String>,
}

pub fn maybe_convert_iclr(input: &str) -> Option<String> {
    let root = parse(input);
    let (show, name) = find_show_rule_with_prefix(&root, "iclr")?;
    let args = extract_named_args(&show);
    let lets = crate::template_adapters::common::collect_let_bindings(&root);

    let title = args
        .get("title")
        .and_then(|node| extract_string_like(node, &lets));
    let abstract_text = args
        .get("abstract")
        .and_then(|node| extract_string_like(node, &lets));
    let accepted = args
        .get("accepted")
        .and_then(|node| extract_option_bool(node, &lets))
        .unwrap_or(Some(false));

    let mut author_groups = args
        .get("authors")
        .map(|node| parse_iclr_authors(node, &lets))
        .unwrap_or_default();
    if matches!(accepted, Some(false)) {
        author_groups = vec![IclrAuthorGroup {
            names: vec!["Anonymous authors".to_string()],
            affiliation: None,
            address: None,
            email: None,
        }];
    }

    let year = extract_year_from_name(&name, "iclr").unwrap_or_else(|| "2025".to_string());
    let style_pkg = format!("iclr{}_conference", year);

    let hints = extract_preamble_hints(input);
    let base_font_size_pt =
        hints.text_size.as_deref().and_then(|size| parse_length_to_pt(size, "10pt"));
    let cite_command = Some("citep".to_string());

    let doc = typst_to_ir(input);
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: true,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Top,
            bibliography_style_default: Some("plainnat".to_string()),
            cite_command,
            base_font_size_pt,
        },
    );

    let mut out = String::new();
    out.push_str("\\documentclass{article}\n");
    let fallback = "\\def\\tylaxNoStyle{1}".to_string();
    out.push_str(&format!(
        "\\IfFileExists{{{}.sty}}{{\\usepackage{{{}}}}}{{{}}}\n",
        style_pkg, style_pkg, fallback
    ));
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{booktabs}\n");
    out.push_str("\\usepackage{multirow}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{hyperref}\n");
    out.push_str("\\usepackage{url}\n");
    out.push_str("\\usepackage{natbib}\n");
    out.push_str("\\ifdefined\\tylaxNoStyle\n");
    out.push_str("\\def\\And{\\\\}\n");
    out.push_str("\\fi\n");
    out.push_str("\\providecommand{\\iclrfinalcopy}{}\n");
    if matches!(accepted, Some(true) | None) {
        out.push_str("\\iclrfinalcopy\n");
    }

    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }

    for (name, hex) in &hints.colors {
        out.push_str(&format!(
            "\\definecolor{{{}}}{{HTML}}{{{}}}\n",
            escape_latex(name),
            escape_latex(hex)
        ));
    }

    if let Some(title) = title.as_deref() {
        out.push_str(&format!("\\title{{{}}}\n", escape_latex(title)));
    }
    if !author_groups.is_empty() {
        out.push_str(&render_iclr_authors(&author_groups));
    }

    out.push_str("\\begin{document}\n");
    if title.is_some() || !author_groups.is_empty() {
        out.push_str("\\maketitle\n");
    }
    if let Some(abstract_text) = abstract_text.as_deref() {
        out.push_str("\\begin{abstract}\n");
        out.push_str(&escape_latex(abstract_text));
        out.push_str("\n\\end{abstract}\n");
    }
    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }
    out.push_str("\\end{document}\n");
    Some(out)
}

fn parse_iclr_authors(
    node: &typst_syntax::SyntaxNode,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
) -> Vec<IclrAuthorGroup> {
    let mut out = Vec::new();
    let resolved = resolve_ident(node, lets);
    if resolved.kind() != typst_syntax::SyntaxKind::Array {
        return out;
    }
    for child in extract_array_elements(&resolved) {
        let resolved = resolve_ident(&child, lets);
        if resolved.kind() != typst_syntax::SyntaxKind::Dict {
            continue;
        }
        let mut group = IclrAuthorGroup {
            names: Vec::new(),
            affiliation: None,
            address: None,
            email: None,
        };
        for part in resolved.children() {
            if part.kind() != typst_syntax::SyntaxKind::Named {
                continue;
            }
            let Some(key) = crate::template_adapters::common::extract_named_key(&part) else {
                continue;
            };
            let Some(value) = crate::template_adapters::common::extract_named_value_node(&part)
            else {
                continue;
            };
            match key.as_str() {
                "names" => {
                    if value.kind() == typst_syntax::SyntaxKind::Array {
                        group.names = extract_array_strings(&value, lets);
                    } else if let Some(name) = extract_string_like(&value, lets) {
                        group.names = vec![name];
                    }
                }
                "affilation" | "affiliation" => {
                    group.affiliation = extract_string_like(&value, lets);
                }
                "address" => {
                    group.address = extract_string_like(&value, lets);
                }
                "email" => {
                    group.email = extract_string_like(&value, lets);
                }
                _ => {}
            }
        }
        if !group.names.is_empty() {
            out.push(group);
        }
    }
    out
}

fn render_iclr_authors(groups: &[IclrAuthorGroup]) -> String {
    let mut authors = Vec::new();
    for group in groups {
        let mut lines: Vec<(String, bool)> = Vec::new();
        if let Some(affil) = group.affiliation.as_deref() {
            lines.push((affil.to_string(), false));
        }
        if let Some(address) = group.address.as_deref() {
            lines.push((address.to_string(), false));
        }
        if let Some(email) = group.email.as_deref() {
            lines.push((format!("\\texttt{{{}}}", escape_latex(email)), true));
        }
        let mut block = String::new();
        let names = group
            .names
            .iter()
            .map(|n| escape_latex(n))
            .collect::<Vec<_>>()
            .join(", ");
        block.push_str(&names);
        for (line, raw) in lines {
            block.push_str(" \\\\\n");
            if raw {
                block.push_str(&line);
            } else {
                block.push_str(&escape_latex(&line));
            }
        }
        authors.push(block);
    }
    let mut out = String::new();
    out.push_str("\\author{\n");
    for (idx, block) in authors.iter().enumerate() {
        if idx > 0 {
            out.push_str("\\And\n");
        }
        out.push_str(block);
        out.push('\n');
    }
    out.push_str("}\n");
    out
}
