use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::{parse, SyntaxKind, SyntaxNode};

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints,
    parse_length_to_pt, render_amsthm_definitions,
};
use crate::template_adapters::common::{
    extract_bibliography_path, extract_named_args, find_show_rule_with_prefix,
};

#[derive(Debug, Clone)]
struct ElsevierAuthor {
    name: String,
    affiliation: Option<String>,
    email: Option<String>,
    corresponding: bool,
}

#[derive(Debug, Clone)]
struct ElsevierMetadata {
    title: Option<String>,
    abstract_text: Option<String>,
    keywords: Vec<String>,
    authors: Vec<ElsevierAuthor>,
    journal: Option<String>,
    format: Option<String>, // "preprint", "review", "1p", "3p", "5p"
    bibliography: Option<String>,
}

pub fn maybe_convert_elsevier(input: &str) -> Option<String> {
    let root = parse(input);
    // Match both "elsevier" and "elsearticle" (the @preview package name)
    let (show, _name) = find_show_rule_with_prefix(&root, "elsevier")
        .or_else(|| find_show_rule_with_prefix(&root, "elsearticle"))?;
    let args = extract_named_args(&show);
    let hints = extract_preamble_hints(input);
    let base_font_size_pt = hints
        .text_size
        .as_deref()
        .and_then(|size| parse_length_to_pt(size, "10pt"));

    let meta = extract_metadata(&args);

    // Determine document class options
    let format = meta.format.as_deref().unwrap_or("preprint");
    let class_opts = match format {
        "review" => "review",
        "1p" => "1p",
        "3p" => "3p",
        "5p" => "5p,twocolumn",
        _ => "preprint",
    };

    // Convert body using IR pipeline
    let doc = typst_to_ir(input);
    let two_column = format == "5p";
    let cite_command = hints.cite_command.clone().or_else(|| {
        if hints.uses_natbib {
            Some("citep".to_string())
        } else {
            Some("cite".to_string())
        }
    });
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Booktabs,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Top,
            bibliography_style_default: Some("elsarticle-num".to_string()),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    );

    let mut out = String::new();
    out.push_str(&format!("\\documentclass[{}]{{elsarticle}}\n", class_opts));
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{booktabs}\n");
    out.push_str("\\usepackage{lineno}\n");
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

    // Journal name
    if let Some(journal) = meta.journal.as_deref() {
        out.push_str(&format!("\\journal{{{}}}\n", escape_latex(journal)));
    }

    out.push_str("\\begin{document}\n");
    out.push_str("\\begin{frontmatter}\n");

    // Title
    if let Some(title) = meta.title.as_deref() {
        out.push_str(&format!("\\title{{{}}}\n", escape_latex(title)));
    }

    // Authors
    for author in &meta.authors {
        out.push_str(&render_elsevier_author(author));
    }

    // Abstract
    if let Some(abstract_text) = meta.abstract_text.as_deref() {
        out.push_str("\\begin{abstract}\n");
        out.push_str(&escape_latex(abstract_text));
        out.push_str("\n\\end{abstract}\n");
    }

    // Keywords
    if !meta.keywords.is_empty() {
        out.push_str("\\begin{keyword}\n");
        let kws: Vec<String> = meta.keywords.iter().map(|k| escape_latex(k)).collect();
        out.push_str(&kws.join(" \\sep "));
        out.push_str("\n\\end{keyword}\n");
    }

    out.push_str("\\end{frontmatter}\n");

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }
    // Add bibliography if specified
    if let Some(bib_path) = meta.bibliography.as_deref() {
        let bib_name = bib_path.trim_end_matches(".bib");
        out.push_str("\\bibliographystyle{elsarticle-num}\n");
        out.push_str(&format!("\\bibliography{{{}}}\n", bib_name));
    }
    out.push_str("\\end{document}\n");
    Some(out)
}

fn extract_metadata(
    args: &std::collections::HashMap<String, SyntaxNode>,
) -> ElsevierMetadata {
    let mut meta = ElsevierMetadata {
        title: None,
        abstract_text: None,
        keywords: Vec::new(),
        authors: Vec::new(),
        journal: None,
        format: None,
        bibliography: None,
    };

    if let Some(node) = args.get("title") {
        meta.title = extract_string_like(node);
    }
    if let Some(node) = args.get("abstract") {
        meta.abstract_text = extract_string_like(node);
    }
    if let Some(node) = args.get("keywords") {
        meta.keywords = extract_array_strings(node);
    }
    if let Some(node) = args.get("authors") {
        meta.authors = extract_authors(node);
    }
    // "author" is also accepted (used by @preview/elsearticle)
    if meta.authors.is_empty() {
        if let Some(node) = args.get("author") {
            meta.authors = extract_authors(node);
        }
    }
    if let Some(node) = args.get("journal") {
        meta.journal = extract_string_like(node);
    }
    if let Some(node) = args.get("format") {
        meta.format = extract_string_like(node);
    }
    if let Some(node) = args.get("bibliography") {
        meta.bibliography = extract_bibliography_path(node);
    }

    meta
}

fn extract_authors(node: &SyntaxNode) -> Vec<ElsevierAuthor> {
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

fn parse_author_dict(node: &SyntaxNode) -> Option<ElsevierAuthor> {
    let mut author = ElsevierAuthor {
        name: String::new(),
        affiliation: None,
        email: None,
        corresponding: false,
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
            "corresponding" => {
                let val = extract_string_like(&value).unwrap_or_default();
                author.corresponding = val == "true" || val == "1";
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

fn extract_array_strings(node: &SyntaxNode) -> Vec<String> {
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
            _ => {}
        }
        if let Some(value) = extract_string_like(&child) {
            out.push(value);
        }
    }
    out
}

fn extract_string_like(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Text => Some(node.text().to_string()),
        SyntaxKind::Ident => Some(node.text().to_string()),
        SyntaxKind::Bool => Some(node.text().to_string()),
        SyntaxKind::ContentBlock | SyntaxKind::Markup => Some(extract_markup_text(node)),
        SyntaxKind::Array => {
            let values = extract_array_strings(node);
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        }
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

fn render_elsevier_author(author: &ElsevierAuthor) -> String {
    let mut out = String::new();

    // Author with optional affiliation reference
    out.push_str("\\author");
    if author.corresponding {
        out.push_str("[1]");
    }
    out.push_str(&format!("{{{}}}\n", escape_latex(&author.name)));

    // Email for corresponding author
    if author.corresponding {
        if let Some(ref email) = author.email {
            out.push_str(&format!("\\ead{{{}}}\n", escape_latex(email)));
        }
    }

    // Affiliation
    if let Some(ref affil) = author.affiliation {
        out.push_str(&format!("\\address{{{}}}\n", escape_latex(affil)));
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
