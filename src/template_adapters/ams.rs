use typst_syntax::{parse, SyntaxKind, SyntaxNode};
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, is_two_column,
    render_amsthm_definitions,
};

#[derive(Debug, Default)]
struct AuthorMeta {
    name: String,
    department: Option<String>,
    organization: Option<String>,
    location: Option<String>,
    email: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Default)]
struct AmsMeta {
    title: Option<String>,
    abstract_text: Option<String>,
    bibliography: Option<String>,
    authors: Vec<AuthorMeta>,
}

pub fn maybe_convert_ams(input: &str) -> Option<String> {
    let root = parse(input);
    let show = find_show_with(&root, "ams-article.with")?;
    let meta = extract_meta(&show);

    let doc = typst_to_ir(input);
    let hints = extract_preamble_hints(input);
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: is_two_column(&hints),
            inline_wide_tables: false,
            bibliography_style_default: hints.bibliography_style.clone(),
        },
    );

    let mut out = String::new();
    out.push_str("\\documentclass{amsart}\n");
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage{amsthm}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{hyperref}\n");
    if hints.uses_natbib {
        out.push_str("\\usepackage{natbib}\n");
    }
    if hints.uses_amsthm {
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }
    out.push_str("\\begin{document}\n");

    if let Some(title) = meta.title.as_deref() {
        out.push_str("\\title{");
        out.push_str(&escape_latex(title));
        out.push_str("}\n");
    }

    for author in &meta.authors {
        if !author.name.is_empty() {
            out.push_str("\\author{");
            out.push_str(&escape_latex(&author.name));
            out.push_str("}\n");
        }
        let mut address_parts = Vec::new();
        if let Some(dep) = author.department.as_deref() {
            address_parts.push(dep);
        }
        if let Some(org) = author.organization.as_deref() {
            address_parts.push(org);
        }
        if let Some(loc) = author.location.as_deref() {
            address_parts.push(loc);
        }
        if !address_parts.is_empty() {
            out.push_str("\\address{");
            out.push_str(&escape_latex(&address_parts.join(", ")));
            out.push_str("}\n");
        }
        if let Some(email) = author.email.as_deref() {
            out.push_str("\\email{");
            out.push_str(&escape_latex(email));
            out.push_str("}\n");
        }
        if let Some(url) = author.url.as_deref() {
            out.push_str("\\url{");
            out.push_str(&escape_latex(url));
            out.push_str("}\n");
        }
    }

    out.push_str("\\maketitle\n");

    if let Some(abstract_text) = meta.abstract_text.as_deref() {
        out.push_str("\\begin{abstract}\n");
        out.push_str(&escape_latex(abstract_text));
        out.push_str("\n\\end{abstract}\n");
    }

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }

    if let Some(bib) = meta.bibliography.as_deref() {
        out.push_str("\\bibliographystyle{plain}\n");
        out.push_str("\\bibliography{");
        out.push_str(&escape_latex(bib));
        out.push_str("}\n");
    }

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

fn extract_meta(show_rule: &SyntaxNode) -> AmsMeta {
    let mut meta = AmsMeta::default();
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
            "title" => meta.title = extract_string_like(&value),
            "abstract" => meta.abstract_text = extract_string_like(&value),
            "bibliography" => meta.bibliography = extract_bibliography(&value),
            "authors" => meta.authors = extract_authors(&value),
            _ => {}
        }
    }

    meta
}

fn extract_authors(node: &SyntaxNode) -> Vec<AuthorMeta> {
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

fn parse_author_dict(node: &SyntaxNode) -> Option<AuthorMeta> {
    let mut author = AuthorMeta::default();
    for child in node.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child)?;
        let value = extract_named_value_node(&child)?;
        match key.as_str() {
            "name" => author.name = extract_string_like(&value).unwrap_or_default(),
            "department" => author.department = extract_string_like(&value),
            "organization" => author.organization = extract_string_like(&value),
            "location" => author.location = extract_string_like(&value),
            "email" => author.email = extract_string_like(&value),
            "url" => author.url = extract_string_like(&value),
            _ => {}
        }
    }
    Some(author)
}

fn extract_bibliography(node: &SyntaxNode) -> Option<String> {
    if node.kind() == SyntaxKind::FuncCall {
        let name = func_call_name(node)?;
        if name == "bibliography" {
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    if child.kind() == SyntaxKind::Str {
                        let raw = child.text().trim_matches('"');
                        let trimmed = raw.trim_end_matches(".bib");
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
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
