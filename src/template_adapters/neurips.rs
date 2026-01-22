use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::parse;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, parse_length_to_pt,
    render_amsthm_definitions,
};
use crate::template_adapters::common::{
    escape_latex, extract_named_args, extract_option_bool, extract_string_like,
    extract_year_from_name, find_show_rule_with_prefix, parse_authors_with_affls,
    render_authors_simple,
};

pub fn maybe_convert_neurips(input: &str) -> Option<String> {
    let root = parse(input);
    let (show, name) = find_show_rule_with_prefix(&root, "neurips")?;
    let args = extract_named_args(&show);
    let lets = crate::template_adapters::common::collect_let_bindings(&root);

    let title = args
        .get("title")
        .and_then(|node| extract_string_like(node, &lets));
    let abstract_text = args
        .get("abstract")
        .and_then(|node| extract_string_like(node, &lets));
    let (authors, affls) = args
        .get("authors")
        .map(|node| parse_authors_with_affls(node, &lets))
        .unwrap_or_default();
    let accepted = args
        .get("accepted")
        .and_then(|node| extract_option_bool(node, &lets))
        .unwrap_or(Some(false));

    let year = extract_year_from_name(&name, "neurips").unwrap_or_else(|| "2025".to_string());
    let style_pkg = format!("neurips_{}", year);

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
            two_column: true,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Top,
            bibliography_style_default: Some("plainnat".to_string()),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    );

    let mut out = String::new();
    out.push_str("\\documentclass{article}\n");
    let pkg_line = match accepted {
        Some(true) => format!("\\usepackage[final]{{{}}}", style_pkg),
        None => format!("\\usepackage[preprint]{{{}}}", style_pkg),
        Some(false) => format!("\\usepackage{{{}}}", style_pkg),
    };
    let fallback = "\\def\\tylaxNoStyle{1}".to_string();
    out.push_str(&format!(
        "\\IfFileExists{{{}.sty}}{{{}}}{{{}}}\n",
        style_pkg, pkg_line, fallback
    ));
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{booktabs}\n");
    out.push_str("\\usepackage{multirow}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{hyperref}\n");
    out.push_str("\\usepackage{natbib}\n");
    out.push_str("\\ifdefined\\tylaxNoStyle\n");
    out.push_str("\\def\\And{\\\\}\n");
    out.push_str("\\fi\n");

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
    if !authors.is_empty() {
        out.push_str(&render_authors_simple(&authors, &affls, "\\And"));
    }

    out.push_str("\\begin{document}\n");
    if title.is_some() || !authors.is_empty() {
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
