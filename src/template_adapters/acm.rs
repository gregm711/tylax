use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::parse;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, parse_length_to_pt,
    render_amsthm_definitions,
};
use crate::template_adapters::common::{
    collect_let_bindings, escape_latex, extract_array_elements, extract_array_strings,
    extract_bibliography_path, extract_dict_entries, extract_named_args, extract_string_like,
    find_show_rule_with_prefix, resolve_ident,
};

#[derive(Debug, Clone)]
struct AcmAuthor {
    name: String,
    affiliation: Option<String>,
    email: Option<String>,
    orcid: Option<String>,
}

#[derive(Debug, Default)]
struct AcmConference {
    name: Option<String>,
    short: Option<String>,
    year: Option<String>,
    date: Option<String>,
    venue: Option<String>,
}

#[derive(Debug, Default)]
struct AcmMetadata {
    title: Option<String>,
    subtitle: Option<String>,
    abstract_text: Option<String>,
    authors: Vec<AcmAuthor>,
    keywords: Vec<String>,
    ccs_concepts: Vec<String>,
    acm_format: Option<String>,
    copyright: Option<String>,
    doi: Option<String>,
    isbn: Option<String>,
    conference: AcmConference,
    acm_year: Option<String>,
    bibliography: Option<String>,
}

pub fn maybe_convert_acm(input: &str) -> Option<String> {
    let root = parse(input);
    let (show, _name) = find_show_rule_with_prefix(&root, "acm")?;
    let args = extract_named_args(&show);
    let lets = collect_let_bindings(&root);

    let meta = extract_acm_metadata(&args, &lets);
    let hints = extract_preamble_hints(input);
    let base_font_size_pt = hints
        .text_size
        .as_deref()
        .and_then(|size| parse_length_to_pt(size, "10pt"));

    let cite_command = hints.cite_command.clone().or_else(|| {
        if hints.uses_natbib {
            Some("citep".to_string())
        } else {
            None
        }
    });

    let doc = typst_to_ir(input);
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: true, // ACM sigconf is two-column
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Top,
            bibliography_style_default: Some("ACM-Reference-Format".to_string()),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    );

    let format = meta.acm_format.as_deref().unwrap_or("sigconf");
    let mut out = String::new();

    // Pass xcolor options before documentclass (acmart loads xcolor internally)
    out.push_str("\\PassOptionsToPackage{table}{xcolor}\n");
    // Document class with format
    out.push_str(&format!("\\documentclass[{}]{{acmart}}\n", format));

    // ACM-specific metadata
    if let Some(doi) = meta.doi.as_deref() {
        out.push_str(&format!("\\acmDOI{{{}}}\n", escape_latex(doi)));
    }
    if let Some(isbn) = meta.isbn.as_deref() {
        out.push_str(&format!("\\acmISBN{{{}}}\n", escape_latex(isbn)));
    }
    if let Some(year) = meta.acm_year.as_deref() {
        out.push_str(&format!("\\acmYear{{{}}}\n", escape_latex(year)));
    }
    if let Some(copyright) = meta.copyright.as_deref() {
        out.push_str(&format!("\\setcopyright{{{}}}\n", escape_latex(copyright)));
    }

    // Conference info
    if meta.conference.name.is_some() || meta.conference.short.is_some() {
        let name = meta.conference.name.as_deref().unwrap_or("");
        let short = meta
            .conference
            .short
            .as_deref()
            .unwrap_or(meta.conference.name.as_deref().unwrap_or(""));
        let date = meta.conference.date.as_deref().unwrap_or("");
        let venue = meta.conference.venue.as_deref().unwrap_or("");
        out.push_str(&format!(
            "\\acmConference[{}]{{{}}}{{{}}}{{{}}}",
            escape_latex(short),
            escape_latex(name),
            escape_latex(date),
            escape_latex(venue)
        ));
        out.push('\n');
    }

    // Standard packages
    // Note: acmart already loads amsmath, amssymb, and xcolor internally
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{bm}\n");
    out.push_str("\\usepackage{booktabs}\n");

    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }

    // Custom colors
    for (name, hex) in &hints.colors {
        out.push_str(&format!(
            "\\definecolor{{{}}}{{HTML}}{{{}}}\n",
            escape_latex(name),
            escape_latex(hex)
        ));
    }

    // Title and subtitle
    if let Some(title) = meta.title.as_deref() {
        if let Some(subtitle) = meta.subtitle.as_deref() {
            out.push_str(&format!(
                "\\title[{}]{{{}\\\\\\large {}}}\n",
                escape_latex(title),
                escape_latex(title),
                escape_latex(subtitle)
            ));
        } else {
            out.push_str(&format!("\\title{{{}}}\n", escape_latex(title)));
        }
    }

    // Authors with affiliations
    for author in &meta.authors {
        out.push_str(&format!("\\author{{{}}}\n", escape_latex(&author.name)));
        if let Some(orcid) = author.orcid.as_deref() {
            out.push_str(&format!("\\orcid{{{}}}\n", escape_latex(orcid)));
        }
        if let Some(email) = author.email.as_deref() {
            out.push_str(&format!("\\email{{{}}}\n", escape_latex(email)));
        }
        if let Some(affiliation) = author.affiliation.as_deref() {
            out.push_str(&format!(
                "\\affiliation{{\\institution{{{}}}\\country{{}}}}\n",
                escape_latex(affiliation)
            ));
        }
    }

    out.push_str("\\begin{document}\n");

    // Abstract
    if let Some(abstract_text) = meta.abstract_text.as_deref() {
        out.push_str("\\begin{abstract}\n");
        out.push_str(&escape_latex(abstract_text));
        out.push_str("\n\\end{abstract}\n");
    }

    // CCS concepts
    if !meta.ccs_concepts.is_empty() {
        for concept in &meta.ccs_concepts {
            out.push_str(&format!("\\ccsdesc{{{}}}\n", escape_latex(concept)));
        }
    }

    // Keywords
    if !meta.keywords.is_empty() {
        let kw: Vec<String> = meta.keywords.iter().map(|k| escape_latex(k)).collect();
        out.push_str(&format!("\\keywords{{{}}}\n", kw.join(", ")));
    }

    out.push_str("\\maketitle\n");

    // Body
    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }

    // Add bibliography if specified
    if let Some(bib_path) = meta.bibliography.as_deref() {
        let bib_name = bib_path.trim_end_matches(".bib");
        out.push_str("\\bibliographystyle{ACM-Reference-Format}\n");
        out.push_str(&format!("\\bibliography{{{}}}\n", bib_name));
    }

    out.push_str("\\end{document}\n");
    Some(out)
}

fn extract_acm_metadata(
    args: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
) -> AcmMetadata {
    let mut meta = AcmMetadata::default();

    if let Some(node) = args.get("title") {
        meta.title = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("subtitle") {
        meta.subtitle = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("abstract") {
        meta.abstract_text = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("keywords") {
        meta.keywords = extract_array_strings(node, lets);
    }
    if let Some(node) = args.get("ccs-concepts") {
        meta.ccs_concepts = extract_array_strings(node, lets);
    }
    if let Some(node) = args.get("format") {
        meta.acm_format = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("copyright") {
        meta.copyright = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("doi") {
        meta.doi = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("isbn") {
        meta.isbn = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("conference") {
        meta.conference = parse_acm_conference(node, lets);
    }
    if let Some(node) = args.get("year") {
        meta.acm_year = extract_string_like(node, lets);
    }
    if let Some(node) = args.get("bibliography") {
        meta.bibliography = extract_bibliography_path(node);
    }

    // Parse affiliations first (for author lookup)
    let affiliations = args
        .get("affiliations")
        .map(|node| parse_acm_affiliations(node, lets))
        .unwrap_or_default();

    // Parse authors and link to affiliations
    if let Some(node) = args.get("authors") {
        meta.authors = parse_acm_authors(node, lets, &affiliations);
    }

    meta
}

fn parse_acm_conference(
    node: &typst_syntax::SyntaxNode,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
) -> AcmConference {
    use typst_syntax::SyntaxKind;
    let mut conf = AcmConference::default();

    let resolved = resolve_ident(node, lets);
    if resolved.kind() != SyntaxKind::Dict {
        // Fallback: treat as string for backwards compatibility
        if let Some(name) = extract_string_like(node, lets) {
            conf.name = Some(name);
        }
        return conf;
    }

    for (key, value) in extract_dict_entries(&resolved, lets) {
        match key.as_str() {
            "name" => conf.name = extract_string_like(&value, lets),
            "short" => conf.short = extract_string_like(&value, lets),
            "year" => conf.year = extract_string_like(&value, lets),
            "date" => conf.date = extract_string_like(&value, lets),
            "venue" | "location" => conf.venue = extract_string_like(&value, lets),
            _ => {}
        }
    }
    conf
}

#[derive(Debug, Clone, Default)]
struct AcmAffiliation {
    mark: Option<String>,
    name: Option<String>,
    department: Option<String>,
}

fn parse_acm_affiliations(
    node: &typst_syntax::SyntaxNode,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
) -> Vec<AcmAffiliation> {
    use typst_syntax::SyntaxKind;
    let mut affiliations = Vec::new();

    let resolved = resolve_ident(node, lets);
    if resolved.kind() != SyntaxKind::Array {
        return affiliations;
    }

    for child in extract_array_elements(&resolved) {
        let resolved_child = resolve_ident(&child, lets);
        if resolved_child.kind() == SyntaxKind::Dict {
            let mut affl = AcmAffiliation::default();
            for (key, value) in extract_dict_entries(&resolved_child, lets) {
                match key.as_str() {
                    "mark" => affl.mark = extract_string_like(&value, lets),
                    "name" | "institution" | "organization" => {
                        affl.name = extract_string_like(&value, lets)
                    }
                    "department" => affl.department = extract_string_like(&value, lets),
                    _ => {}
                }
            }
            affiliations.push(affl);
        }
    }
    affiliations
}

fn parse_acm_authors(
    node: &typst_syntax::SyntaxNode,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
    affiliations: &[AcmAffiliation],
) -> Vec<AcmAuthor> {
    use typst_syntax::SyntaxKind;

    let mut authors = Vec::new();

    let resolved = resolve_ident(node, lets);
    if resolved.kind() != SyntaxKind::Array {
        // Single author as string or dict
        if let Some(author) = parse_single_acm_author(&resolved, lets, affiliations) {
            authors.push(author);
        }
        return authors;
    }

    for child in extract_array_elements(&resolved) {
        let resolved_child = resolve_ident(&child, lets);
        match resolved_child.kind() {
            SyntaxKind::Dict => {
                if let Some(author) = parse_acm_author_dict(&resolved_child, lets, affiliations) {
                    authors.push(author);
                }
            }
            SyntaxKind::Str | SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                if let Some(name) = extract_string_like(&resolved_child, lets) {
                    authors.push(AcmAuthor {
                        name,
                        affiliation: None,
                        email: None,
                        orcid: None,
                    });
                }
            }
            _ => {}
        }
    }

    authors
}

fn parse_single_acm_author(
    node: &typst_syntax::SyntaxNode,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
    affiliations: &[AcmAffiliation],
) -> Option<AcmAuthor> {
    use typst_syntax::SyntaxKind;

    match node.kind() {
        SyntaxKind::Dict => parse_acm_author_dict(node, lets, affiliations),
        SyntaxKind::Str | SyntaxKind::ContentBlock | SyntaxKind::Markup => {
            let name = extract_string_like(node, lets)?;
            Some(AcmAuthor {
                name,
                affiliation: None,
                email: None,
                orcid: None,
            })
        }
        _ => None,
    }
}

fn parse_acm_author_dict(
    node: &typst_syntax::SyntaxNode,
    lets: &std::collections::HashMap<String, typst_syntax::SyntaxNode>,
    affiliations: &[AcmAffiliation],
) -> Option<AcmAuthor> {
    let mut author = AcmAuthor {
        name: String::new(),
        affiliation: None,
        email: None,
        orcid: None,
    };

    let mut author_mark: Option<String> = None;

    for (key, value) in extract_dict_entries(node, lets) {
        match key.as_str() {
            "name" => author.name = extract_string_like(&value, lets).unwrap_or_default(),
            "affiliation" | "institution" | "organization" => {
                author.affiliation = extract_string_like(&value, lets)
            }
            "email" => author.email = extract_string_like(&value, lets),
            "orcid" => author.orcid = extract_string_like(&value, lets),
            "mark" => author_mark = extract_string_like(&value, lets),
            _ => {}
        }
    }

    // Link to affiliation by mark if available and no direct affiliation set
    if author.affiliation.is_none() {
        if let Some(mark) = author_mark {
            for affl in affiliations {
                if affl.mark.as_ref() == Some(&mark) {
                    // Combine department and name for affiliation string
                    let mut parts = Vec::new();
                    if let Some(dept) = &affl.department {
                        parts.push(dept.clone());
                    }
                    if let Some(name) = &affl.name {
                        parts.push(name.clone());
                    }
                    if !parts.is_empty() {
                        author.affiliation = Some(parts.join(", "));
                    }
                    break;
                }
            }
        }
    }

    if author.name.is_empty() {
        None
    } else {
        Some(author)
    }
}

