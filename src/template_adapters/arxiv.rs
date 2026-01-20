use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_numbering_enabled, extract_preamble_hints, is_two_column, parse_length_to_pt,
    render_article_preamble,
};

pub fn maybe_convert_arxiv(input: &str) -> Option<String> {
    if !input.contains("arXiv Preprint Template") {
        return None;
    }

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
    let preamble = render_article_preamble(&hints);

    let mut out = String::new();
    out.push_str(&preamble);
    out.push_str("\\begin{document}\n\n");

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }

    out.push_str("\\end{document}\n");
    Some(out)
}
