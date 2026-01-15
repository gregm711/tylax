use typst_syntax::parse;
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, parse_length_to_pt,
    render_amsthm_definitions,
};
use crate::template_adapters::common::{
    escape_latex, extract_named_args, extract_option_bool, extract_string_like,
    find_show_rule_with_prefix, parse_authors_with_affls, render_authors_simple,
};

pub fn maybe_convert_tmlr(input: &str) -> Option<String> {
    let root = parse(input);
    let (show, _name) = find_show_rule_with_prefix(&root, "tmlr")?;
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
    let review = args
        .get("review")
        .and_then(|node| extract_string_like(node, &lets));

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
            two_column: false,
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
    let pkg_line = match accepted {
        Some(true) => "\\usepackage[accepted]{tmlr}".to_string(),
        None => "\\usepackage[preprint]{tmlr}".to_string(),
        Some(false) => "\\usepackage{tmlr}".to_string(),
    };
    out.push_str(&format!(
        "\\IfFileExists{{tmlr.sty}}{{{}}}{{}}\n",
        pkg_line
    ));
    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{booktabs}\n");
    out.push_str("\\usepackage{multirow}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{hyperref}\n");
    out.push_str("\\usepackage{url}\n");
    out.push_str("\\usepackage{natbib}\n");
    out.push_str("\\providecommand{\\reviewurl}[1]{}\n");
    if let Some(review) = review.as_deref() {
        out.push_str(&format!("\\reviewurl{{{}}}\n", escape_latex(review)));
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
    if !authors.is_empty() {
        out.push_str(&render_authors_simple(&authors, &affls, "\\and"));
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
