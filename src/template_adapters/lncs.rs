use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::{parse, SyntaxKind, SyntaxNode};

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints,
    parse_length_to_pt, render_amsthm_definitions,
};
use crate::template_adapters::common::{
    collect_let_bindings, extract_bibliography_path, extract_named_args, find_show_rule_with_prefix,
};

#[derive(Debug, Clone)]
struct LncsAuthor {
    name: String,
    institute: Option<usize>, // institute index (1-based)
    orcid: Option<String>,
    email: Option<String>,
}

#[derive(Debug, Clone)]
struct LncsInstitute {
    name: String,
    city: Option<String>,
    country: Option<String>,
}

#[derive(Debug, Clone)]
struct LncsMetadata {
    title: Option<String>,
    subtitle: Option<String>,
    abstract_text: Option<String>,
    keywords: Vec<String>,
    authors: Vec<LncsAuthor>,
    institutes: Vec<LncsInstitute>,
    bibliography: Option<String>,
}

pub fn maybe_convert_lncs(input: &str) -> Option<String> {
    let root = parse(input);
    let (show, _name) = find_show_rule_with_prefix(&root, "lncs")?;
    let args = extract_named_args(&show);
    let lets = collect_let_bindings(&root);
    let hints = extract_preamble_hints(input);
    let base_font_size_pt = hints
        .text_size
        .as_deref()
        .and_then(|size| parse_length_to_pt(size, "10pt"));

    let meta = extract_metadata(&args, &lets);

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
            two_column: false,
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
    out.push_str("\\documentclass[runningheads]{llncs}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{booktabs}\n");
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

    out.push_str("\\begin{document}\n");

    // Title
    if let Some(title) = meta.title.as_deref() {
        out.push_str(&format!("\\title{{{}}}\n", escape_latex(title)));
    }
    if let Some(subtitle) = meta.subtitle.as_deref() {
        out.push_str(&format!("\\subtitle{{{}}}\n", escape_latex(subtitle)));
    }

    // Authors and institutes
    if !meta.authors.is_empty() {
        out.push_str(&render_lncs_authors(&meta.authors));
    }
    if !meta.institutes.is_empty() {
        out.push_str(&render_lncs_institutes(&meta.institutes));
    }

    out.push_str("\\maketitle\n");

    // Abstract
    if let Some(abstract_text) = meta.abstract_text.as_deref() {
        out.push_str("\\begin{abstract}\n");
        out.push_str(&escape_latex(abstract_text));
        out.push_str("\n");
        if !meta.keywords.is_empty() {
            out.push_str("\\keywords{");
            let kws: Vec<String> = meta.keywords.iter().map(|k| escape_latex(k)).collect();
            out.push_str(&kws.join(" \\and "));
            out.push_str("}\n");
        }
        out.push_str("\\end{abstract}\n");
    }

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }
    // Add bibliography if specified
    if let Some(bib_path) = meta.bibliography.as_deref() {
        let bib_name = bib_path.trim_end_matches(".bib");
        out.push_str("\\bibliographystyle{splncs04}\n");
        out.push_str(&format!("\\bibliography{{{}}}\n", bib_name));
    }
    out.push_str("\\end{document}\n");
    Some(out)
}

fn extract_metadata(
    args: &std::collections::HashMap<String, SyntaxNode>,
    lets: &std::collections::HashMap<String, SyntaxNode>,
) -> LncsMetadata {
    let mut meta = LncsMetadata {
        title: None,
        subtitle: None,
        abstract_text: None,
        keywords: Vec::new(),
        authors: Vec::new(),
        institutes: Vec::new(),
        bibliography: None,
    };

    if let Some(node) = args.get("title") {
        meta.title = extract_string_like(node);
    }
    if let Some(node) = args.get("subtitle") {
        meta.subtitle = extract_string_like(node);
    }
    if let Some(node) = args.get("abstract") {
        meta.abstract_text = extract_string_like(node);
    }
    if let Some(node) = args.get("keywords") {
        meta.keywords = extract_array_strings(node);
    }
    if let Some(node) = args.get("authors") {
        meta.authors = extract_authors(node, lets);
    }
    if let Some(node) = args.get("institutes") {
        meta.institutes = extract_institutes(node);
    }
    if let Some(node) = args.get("bibliography") {
        meta.bibliography = extract_bibliography_path(node);
    }

    meta
}

fn extract_authors(
    node: &SyntaxNode,
    lets: &std::collections::HashMap<String, SyntaxNode>,
) -> Vec<LncsAuthor> {
    let mut authors = Vec::new();
    if node.kind() != SyntaxKind::Array {
        return authors;
    }
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Dict => {
                if let Some(author) = parse_author_dict(&child) {
                    authors.push(author);
                }
            }
            SyntaxKind::FuncCall => {
                // Handle author("Name", insts: ...) function calls from fine-lncs
                if let Some(author) = parse_author_func_call(&child, lets) {
                    authors.push(author);
                }
            }
            _ => {}
        }
    }
    authors
}

fn parse_author_func_call(
    node: &SyntaxNode,
    lets: &std::collections::HashMap<String, SyntaxNode>,
) -> Option<LncsAuthor> {
    let mut author = LncsAuthor {
        name: String::new(),
        institute: None,
        orcid: None,
        email: None,
    };

    // Get the args from the function call
    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;

    for child in args.children() {
        match child.kind() {
            SyntaxKind::Str => {
                // First positional string argument is the name
                if author.name.is_empty() {
                    author.name = child.text().trim_matches('"').to_string();
                }
            }
            SyntaxKind::Named => {
                let key = extract_named_key(&child)?;
                let value = extract_named_value_node(&child)?;
                match key.as_str() {
                    "insts" => {
                        // Extract institute names from the insts tuple
                        // insts: (tu-berlin,) where tu-berlin is a let-bound institute
                        if let Some(inst_name) = extract_first_institute(&value, lets) {
                            // For simplicity, we'll just store the first institute
                            // Real implementation would track multiple
                            author.institute = Some(1);
                            // Store the name in case we need it
                            let _ = inst_name;
                        }
                    }
                    "orcid" => {
                        author.orcid = extract_string_like(&value);
                    }
                    "email" => {
                        author.email = extract_string_like(&value);
                    }
                    _ => {}
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

fn extract_first_institute(
    node: &SyntaxNode,
    _lets: &std::collections::HashMap<String, SyntaxNode>,
) -> Option<String> {
    // Handle tuple like (tu-berlin,)
    if node.kind() == SyntaxKind::Array {
        for child in node.children() {
            if child.kind() == SyntaxKind::Ident {
                return Some(child.text().to_string());
            }
        }
    }
    None
}

fn parse_author_dict(node: &SyntaxNode) -> Option<LncsAuthor> {
    let mut author = LncsAuthor {
        name: String::new(),
        institute: None,
        orcid: None,
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
            "institute" => {
                if let Some(s) = extract_string_like(&value) {
                    author.institute = s.parse().ok();
                }
            }
            "orcid" => {
                author.orcid = extract_string_like(&value);
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

fn extract_institutes(node: &SyntaxNode) -> Vec<LncsInstitute> {
    let mut institutes = Vec::new();
    if node.kind() != SyntaxKind::Array {
        return institutes;
    }
    for child in node.children() {
        if child.kind() == SyntaxKind::Dict {
            if let Some(inst) = parse_institute_dict(&child) {
                institutes.push(inst);
            }
        }
    }
    institutes
}

fn parse_institute_dict(node: &SyntaxNode) -> Option<LncsInstitute> {
    let mut inst = LncsInstitute {
        name: String::new(),
        city: None,
        country: None,
    };

    for child in node.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child)?;
        let value = extract_named_value_node(&child)?;
        match key.as_str() {
            "name" => {
                inst.name = extract_string_like(&value).unwrap_or_default();
            }
            "city" => {
                inst.city = extract_string_like(&value);
            }
            "country" => {
                inst.country = extract_string_like(&value);
            }
            _ => {}
        }
    }

    if inst.name.is_empty() {
        None
    } else {
        Some(inst)
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
        SyntaxKind::Int => Some(node.text().to_string()),
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

fn render_lncs_authors(authors: &[LncsAuthor]) -> String {
    let mut out = String::new();
    let author_strs: Vec<String> = authors
        .iter()
        .map(|a| {
            let mut s = escape_latex(&a.name);
            if let Some(inst) = a.institute {
                s.push_str(&format!("\\inst{{{}}}", inst));
            }
            if let Some(ref orcid) = a.orcid {
                s.push_str(&format!("\\orcidID{{{}}}", escape_latex(orcid)));
            }
            s
        })
        .collect();
    out.push_str(&format!("\\author{{{}}}\n", author_strs.join(" \\and ")));

    // Author runner (short names for header)
    let short_names: Vec<String> = authors
        .iter()
        .map(|a| {
            let parts: Vec<&str> = a.name.split_whitespace().collect();
            if parts.len() > 1 {
                format!(
                    "{}. {}",
                    parts[0].chars().next().unwrap_or_default(),
                    parts.last().unwrap_or(&"")
                )
            } else {
                a.name.clone()
            }
        })
        .collect();
    out.push_str(&format!(
        "\\authorrunning{{{}}}\n",
        escape_latex(&short_names.join(", "))
    ));

    out
}

fn render_lncs_institutes(institutes: &[LncsInstitute]) -> String {
    let mut out = String::new();
    let inst_strs: Vec<String> = institutes
        .iter()
        .map(|i| {
            let mut parts = vec![escape_latex(&i.name)];
            if let Some(ref city) = i.city {
                parts.push(escape_latex(city));
            }
            if let Some(ref country) = i.country {
                parts.push(escape_latex(country));
            }
            parts.join(", ")
        })
        .collect();
    out.push_str(&format!("\\institute{{{}}}\n", inst_strs.join(" \\and ")));
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
