use typst_syntax::{parse, SyntaxKind, SyntaxNode};
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, is_two_column,
    render_amsthm_definitions,
};

#[derive(Debug, Clone)]
struct IeeeAuthor {
    name: String,
    department: Option<String>,
    organization: Option<String>,
    location: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Clone)]
struct IeeeMetadata {
    title: Option<String>,
    abstract_text: Option<String>,
    index_terms: Vec<String>,
    authors: Vec<IeeeAuthor>,
}

pub fn maybe_convert_ieee(input: &str) -> Option<String> {
    let root = parse(input);
    let show = find_ieee_show_rule(&root)?;
    let meta = extract_metadata(&show);
    let hints = extract_preamble_hints(input);

    // Convert body using IR pipeline (show/let/set are ignored by preprocessor).
    let doc = typst_to_ir(input);
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: is_two_column(&hints),
            inline_wide_tables: true,
            bibliography_style_default: hints.bibliography_style.clone(),
        },
    );

    let mut out = String::new();
    out.push_str("\\documentclass[conference]{IEEEtran}\n");
    if hints.uses_natbib {
        out.push_str("\\usepackage{natbib}\n");
    } else {
        out.push_str("\\usepackage{cite}\n");
    }
    out.push_str("\\usepackage{amsmath,amssymb,amsfonts}\n");
    out.push_str("\\usepackage{algorithmic}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{textcomp}\n");
    out.push_str("\\usepackage{xcolor}\n");
    out.push_str("\\usepackage{caption}\n");
    if let Some(font) = hints.font.as_deref() {
        if is_new_computer_modern(font) {
            out.push_str("\\usepackage{newcomputermodern}\n");
        } else {
            out.push_str("\\usepackage{iftex}\n");
            out.push_str("\\ifPDFTeX\n");
            out.push_str("\\else\n");
            out.push_str("\\usepackage{fontspec}\n");
            out.push_str(&format!("\\setmainfont{{{}}}\n", escape_latex(font)));
            out.push_str("\\fi\n");
        }
    }
    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }
    out.push_str(&render_ieee_heading_overrides());
    out.push_str("\\providecommand{\\textsubscript}[1]{$_{\\text{#1}}$}\n");
    for (name, hex) in &hints.colors {
        out.push_str(&format!(
            "\\definecolor{{{}}}{{HTML}}{{{}}}\n",
            escape_latex(name),
            escape_latex(hex)
        ));
    }
    out.push_str("\\begin{document}\n");
    out.push_str("\\setlength{\\parindent}{1em}\n");

    out.push_str("\\makeatletter\n");
    out.push_str("\\twocolumn[{\\begin{@twocolumnfalse}\n");
    out.push_str(&render_ieee_title_block(&meta));
    out.push_str("\\end{@twocolumnfalse}}]\n");
    out.push_str("\\makeatother\n");

    if !body.trim().is_empty() {
        let body = wrap_bibliography_size(&body, "9pt", "10.8pt");
        out.push_str(&body);
        out.push('\n');
    }

    out.push_str("\\end{document}\n");
    Some(out)
}

fn find_ieee_show_rule(root: &SyntaxNode) -> Option<SyntaxNode> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::ShowRule {
            if let Some(func) = node.children().find(|c| c.kind() == SyntaxKind::FuncCall) {
                if let Some(name) = func_call_name(&func) {
                    if name == "ieee.with" {
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

fn extract_metadata(show_rule: &SyntaxNode) -> IeeeMetadata {
    let mut meta = IeeeMetadata {
        title: None,
        abstract_text: None,
        index_terms: Vec::new(),
        authors: Vec::new(),
    };

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
            "title" => {
                meta.title = extract_string_like(&value);
            }
            "abstract" => {
                meta.abstract_text = extract_string_like(&value);
            }
            "index-terms" => {
                meta.index_terms = extract_array_strings(&value);
            }
            "authors" => {
                meta.authors = extract_authors(&value);
            }
            _ => {}
        }
    }

    meta
}

fn extract_authors(node: &SyntaxNode) -> Vec<IeeeAuthor> {
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

fn parse_author_dict(node: &SyntaxNode) -> Option<IeeeAuthor> {
    let mut author = IeeeAuthor {
        name: String::new(),
        department: None,
        organization: None,
        location: None,
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
            "department" => {
                author.department = extract_string_like(&value);
            }
            "organization" => {
                author.organization = extract_string_like(&value);
            }
            "location" => {
                author.location = extract_string_like(&value);
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

fn is_new_computer_modern(value: &str) -> bool {
    let lowered = value.trim().trim_matches('"').to_lowercase();
    lowered.contains("new computer modern")
}

fn render_ieee_heading_overrides() -> String {
    let mut out = String::new();
    out.push_str("\\makeatletter\n");
    out.push_str("\\renewcommand\\section{\\@startsection{section}{1}{\\z@}{0.5em}{0.3em}{\\centering\\normalfont\\bfseries\\MakeUppercase}}\n");
    out.push_str("\\renewcommand\\subsection{\\@startsection{subsection}{2}{\\z@}{0.5em}{0.3em}{\\normalfont\\bfseries}}\n");
    out.push_str("\\renewcommand\\subsubsection{\\@startsection{subsubsection}{3}{\\z@}{0pt}{0pt}{\\normalfont\\bfseries}}\n");
    out.push_str("\\makeatother\n");
    out
}

fn render_ieee_title_block(meta: &IeeeMetadata) -> String {
    let mut out = String::new();
    out.push_str("\\begin{center}\n");
    if let Some(title) = meta.title.as_deref() {
        out.push_str("{\\fontsize{24pt}{28pt}\\selectfont\\bfseries ");
        out.push_str(&escape_latex(title));
        out.push_str("}\n");
        out.push_str("\\vspace{1em}\n");
    }

    if !meta.authors.is_empty() {
        out.push_str(&render_ieee_authors_grid(&meta.authors));
    }

    if meta.abstract_text.is_some() || !meta.index_terms.is_empty() {
        out.push_str("\\vspace{1.5em}\n");
        out.push_str("\\begin{minipage}{\\linewidth}\n");
        out.push_str("\\raggedright\n");
        if let Some(abstract_text) = meta.abstract_text.as_deref() {
            out.push_str("\\textbf{Abstract---}");
            out.push_str("\\textit{");
            out.push_str(&escape_latex(abstract_text.trim()));
            out.push_str("}\n");
            if !meta.index_terms.is_empty() {
                out.push_str("\\vspace{0.5em}\n");
            }
        }
        if !meta.index_terms.is_empty() {
            out.push_str("\\textbf{Index Terms---}");
            let terms: Vec<String> = meta
                .index_terms
                .iter()
                .map(|t| escape_latex(t.trim()))
                .collect();
            out.push_str(&terms.join(", "));
            out.push('\n');
        }
        out.push_str("\\end{minipage}\n");
    }

    out.push_str("\\end{center}\n");
    out
}

fn render_ieee_authors_grid(authors: &[IeeeAuthor]) -> String {
    if authors.is_empty() {
        return String::new();
    }
    let cols = authors.len().min(3);
    let mut col_spec = String::new();
    for idx in 0..cols {
        if idx > 0 {
            col_spec.push_str("@{\\hspace{1.5em}}");
        }
        col_spec.push('c');
    }
    let mut out = String::new();
    out.push_str(&format!("\\begin{{tabular}}{{{}}}\n", col_spec));
    for row in authors.chunks(cols) {
        let mut cells: Vec<String> = row
            .iter()
            .map(render_ieee_author_cell)
            .collect();
        while cells.len() < cols {
            cells.push(String::new());
        }
        out.push_str(&cells.join(" & "));
        out.push_str(" \\\\\n");
    }
    out.push_str("\\end{tabular}\n");
    out
}

fn render_ieee_author_cell(author: &IeeeAuthor) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "{{\\fontsize{{11pt}}{{13.2pt}}\\selectfont {}}}",
        escape_latex(&author.name)
    ));
    if let Some(dep) = author.department.as_deref() {
        lines.push(format!(
            "{{\\fontsize{{9pt}}{{10.8pt}}\\selectfont\\textit{{{}}}}}",
            escape_latex(dep)
        ));
    }
    if let Some(org) = author.organization.as_deref() {
        lines.push(format!(
            "{{\\fontsize{{9pt}}{{10.8pt}}\\selectfont {}}}",
            escape_latex(org)
        ));
    }
    if let Some(loc) = author.location.as_deref() {
        lines.push(format!(
            "{{\\fontsize{{9pt}}{{10.8pt}}\\selectfont {}}}",
            escape_latex(loc)
        ));
    }
    if let Some(email) = author.email.as_deref() {
        lines.push(format!(
            "{{\\fontsize{{9pt}}{{10.8pt}}\\selectfont {}}}",
            escape_latex(email)
        ));
    }
    format!(
        "\\begin{{tabular}}{{@{{}}c@{{}}}}{}\\end{{tabular}}",
        lines.join(" \\\\ ")
    )
}

fn wrap_bibliography_size(body: &str, size: &str, baseline: &str) -> String {
    let begin = "\\begin{thebibliography}";
    let end = "\\end{thebibliography}";
    let Some(start) = body.find(begin) else {
        return body.to_string();
    };
    let Some(end_rel) = body[start..].find(end) else {
        return body.to_string();
    };
    let end_idx = start + end_rel + end.len();
    let mut out = String::new();
    out.push_str(&body[..start]);
    out.push_str(&format!("{{\\fontsize{{{}}}{{{}}}\\selectfont\n", size, baseline));
    out.push_str(&body[start..end_idx]);
    out.push_str("\n}\n");
    out.push_str(&body[end_idx..]);
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
