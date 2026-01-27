use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::{parse, SyntaxKind, SyntaxNode};

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints,
    parse_length_to_pt, render_amsthm_definitions,
};
use crate::template_adapters::common::{extract_named_args, find_show_rule_with_prefix};

#[derive(Debug, Clone)]
struct AaaiAuthor {
    name: String,
    affiliation: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Clone)]
struct AaaiMetadata {
    title: Option<String>,
    abstract_text: Option<String>,
    authors: Vec<AaaiAuthor>,
}

pub fn maybe_convert_aaai(input: &str) -> Option<String> {
    let root = parse(input);
    let (show, _name) = find_show_rule_with_prefix(&root, "aaai")?;
    let args = extract_named_args(&show);
    let hints = extract_preamble_hints(input);
    let base_font_size_pt = hints
        .text_size
        .as_deref()
        .and_then(|size| parse_length_to_pt(size, "10pt"));

    let meta = extract_metadata(&args);

    // Convert body using IR pipeline
    let doc = typst_to_ir(input);
    let cite_command = hints.cite_command.clone().or_else(|| {
        if hints.uses_natbib {
            Some("citep".to_string())
        } else {
            None
        }
    });
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: true,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Booktabs,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Top,
            bibliography_style_default: hints.bibliography_style.clone(),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    );

    let mut out = String::new();
    // Use standard article class with AAAI-like formatting
    // (aaai24.sty is not widely available in TeX distributions)
    out.push_str("\\documentclass[letterpaper,twocolumn]{article}\n");
    out.push_str("\\usepackage[margin=1in]{geometry}\n");
    out.push_str("\\usepackage{times}\n");
    out.push_str("\\usepackage{helvet}\n");
    out.push_str("\\usepackage{courier}\n");
    out.push_str("\\usepackage[hyphens]{url}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\urlstyle{rm}\n");
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage{bm}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{booktabs}\n");
    out.push_str("\\usepackage{algorithm}\n");
    out.push_str("\\usepackage{algorithmic}\n");
    out.push_str("\\usepackage{float}\n");
    out.push_str("\\usepackage{hyperref}\n");
    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }
    out.push_str("\\providecommand{\\textsubscript}[1]{$_{\\text{#1}}$}\n");
    for (name, hex) in &hints.colors {
        out.push_str(&format!(
            "\\definecolor{{{}}}{{HTML}}{{{}}}\n",
            escape_latex(name),
            escape_latex(hex)
        ));
    }

    // PDF metadata via hyperref
    out.push_str("\\hypersetup{\n");
    if let Some(title) = meta.title.as_deref() {
        out.push_str(&format!("  pdftitle={{{}}},\n", escape_latex(title)));
    }
    if !meta.authors.is_empty() {
        let names: Vec<String> = meta.authors.iter().map(|a| a.name.clone()).collect();
        out.push_str(&format!("  pdfauthor={{{}}},\n", escape_latex(&names.join(", "))));
    }
    out.push_str("}\n");

    // Title
    if let Some(title) = meta.title.as_deref() {
        out.push_str(&format!("\\title{{{}}}\n", escape_latex(title)));
    }

    // Authors
    if !meta.authors.is_empty() {
        out.push_str("\\author{\n");
        out.push_str(&render_aaai_authors(&meta.authors));
        out.push_str("}\n");
    }

    out.push_str("\\begin{document}\n");
    out.push_str("\\maketitle\n");

    // Abstract
    if let Some(abstract_text) = meta.abstract_text.as_deref() {
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

fn extract_metadata(
    args: &std::collections::HashMap<String, SyntaxNode>,
) -> AaaiMetadata {
    let mut meta = AaaiMetadata {
        title: None,
        abstract_text: None,
        authors: Vec::new(),
    };

    if let Some(node) = args.get("title") {
        meta.title = extract_string_like(node);
    }
    if let Some(node) = args.get("abstract") {
        meta.abstract_text = extract_string_like(node);
    }
    if let Some(node) = args.get("authors") {
        meta.authors = extract_authors(node);
    }

    meta
}

fn extract_authors(node: &SyntaxNode) -> Vec<AaaiAuthor> {
    let mut authors = Vec::new();
    if node.kind() != SyntaxKind::Array {
        return authors;
    }
    for child in node.children() {
        if child.kind() == SyntaxKind::Dict {
            if let Some(author) = parse_author_dict(&child) {
                authors.push(author);
            }
        }
    }
    authors
}

fn parse_author_dict(node: &SyntaxNode) -> Option<AaaiAuthor> {
    let mut author = AaaiAuthor {
        name: String::new(),
        affiliation: None,
        email: None,
    };

    for child in node.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child)?;
        let value = extract_named_value_node(&child)?;
        match key.as_str() {
            "name" => {
                author.name = extract_string_like(&value).unwrap_or_default();
            }
            "affiliation" => {
                author.affiliation = extract_string_like(&value);
            }
            "email" => {
                author.email = extract_string_like(&value);
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

fn extract_string_like(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Text => Some(node.text().to_string()),
        SyntaxKind::Ident => Some(node.text().to_string()),
        SyntaxKind::ContentBlock | SyntaxKind::Markup => Some(extract_markup_text(node)),
        _ => Some(node.clone().into_text().to_string()),
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

fn render_aaai_authors(authors: &[AaaiAuthor]) -> String {
    let mut out = String::new();

    // Group authors by affiliation for cleaner output
    for (idx, author) in authors.iter().enumerate() {
        if idx > 0 {
            out.push_str(" \\and\n");
        }
        out.push_str(&escape_latex(&author.name));
        if let Some(ref affil) = author.affiliation {
            out.push_str(&format!(" \\\\ {}", escape_latex(affil)));
        }
        if let Some(ref email) = author.email {
            out.push_str(&format!(" \\\\ {}", escape_latex(email)));
        }
    }
    out
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

