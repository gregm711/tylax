//! IR-based Typst → LaTeX pipeline.

use tylax_ir::Document;
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::template_adapters::ams::maybe_convert_ams;
use crate::template_adapters::arxiv::maybe_convert_arxiv;
use crate::template_adapters::book::maybe_convert_book;
use crate::template_adapters::ieee::maybe_convert_ieee;
use crate::template_adapters::letter::maybe_convert_letter;
use crate::template_adapters::newsletter::maybe_convert_newsletter;
use crate::preamble_hints::{
    equation_numbering_enabled, extract_preamble_hints, is_two_column, render_article_preamble,
};

/// Convert Typst to LaTeX using the IR pipeline.
pub fn typst_to_latex_ir(input: &str, full_document: bool) -> String {
    if full_document {
        if let Some(rendered) = maybe_convert_ieee(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_ams(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_book(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_letter(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_newsletter(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_arxiv(input) {
            return rendered;
        }
    }
    let doc: Document = typst_to_ir(input);
    if full_document {
        let hints = extract_preamble_hints(input);
        let preamble = render_article_preamble(&hints);
        let number_equations = equation_numbering_enabled(&hints);
        let body = render_document(
            &doc,
            LatexRenderOptions {
                full_document: false,
                number_equations,
                two_column: is_two_column(&hints),
                inline_wide_tables: false,
                table_grid: false,
                bibliography_style_default: hints.bibliography_style.clone(),
            },
        );
        let mut out = String::new();
        out.push_str(&preamble);
        out.push_str("\\begin{document}\n\n");
        if !body.trim().is_empty() {
            out.push_str(&body);
            out.push('\n');
        }
        out.push_str("\\end{document}\n");
        return out;
    }
    let hints = extract_preamble_hints(input);
    render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: is_two_column(&hints),
            inline_wide_tables: false,
            table_grid: false,
            bibliography_style_default: hints.bibliography_style.clone(),
        },
    )
}
