//! IR-based Typst → LaTeX pipeline.

use tylax_ir::Document;
use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;

use crate::preamble_hints::{
    equation_numbering_enabled, extract_preamble_hints, is_two_column, parse_length_to_pt,
    render_article_preamble,
};
use crate::template_adapters::ams::maybe_convert_ams;
use crate::template_adapters::arxiv::maybe_convert_arxiv;
use crate::template_adapters::book::maybe_convert_book;
use crate::template_adapters::cvpr::maybe_convert_cvpr;
use crate::template_adapters::generic::maybe_convert_template_with;
use crate::template_adapters::iclr::maybe_convert_iclr;
use crate::template_adapters::icml::maybe_convert_icml;
use crate::template_adapters::ieee::maybe_convert_ieee;
use crate::template_adapters::jmlr::maybe_convert_jmlr;
use crate::template_adapters::letter::maybe_convert_letter;
use crate::template_adapters::neurips::maybe_convert_neurips;
use crate::template_adapters::newsletter::maybe_convert_newsletter;
use crate::template_adapters::tmlr::maybe_convert_tmlr;
use crate::utils::loss::{ConversionReport, LossRecord, LossReport, LOSS_MARKER_PREFIX};

fn build_loss_report(doc: &Document) -> LossReport {
    let mut records = Vec::new();
    for (idx, loss) in doc.losses.iter().enumerate() {
        let id = format!("L{:04}", idx + 1);
        records.push(LossRecord::from_ir_loss(id, loss));
    }
    LossReport::new("typst", "latex", records, Vec::new())
}

fn append_loss_markers(output: &mut String, report: &LossReport, full_document: bool) {
    if report.losses.is_empty() {
        return;
    }
    let mut marker_block = String::new();
    marker_block.push('\n');
    marker_block.push_str("% Tylax conversion losses\n");
    for loss in &report.losses {
        let name = loss
            .name
            .as_ref()
            .map(|n| format!(" kind={}", n))
            .unwrap_or_default();
        let line = format!(
            "% {}{}{} message={}\n",
            LOSS_MARKER_PREFIX,
            loss.id,
            name,
            loss.message.replace('\n', " ")
        );
        marker_block.push_str(&line);
    }
    if full_document {
        if let Some(pos) = output.rfind("\\end{document}") {
            output.insert_str(pos, &marker_block);
            return;
        }
    }
    output.push_str(&marker_block);
}

/// Convert Typst to LaTeX using the IR pipeline.
pub fn typst_to_latex_ir(input: &str, full_document: bool) -> String {
    if full_document {
        if let Some(rendered) = maybe_convert_ieee(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_neurips(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_icml(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_iclr(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_cvpr(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_tmlr(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_jmlr(input) {
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
        if let Some(rendered) = maybe_convert_template_with(input) {
            return rendered;
        }
        if let Some(rendered) = maybe_convert_arxiv(input) {
            return rendered;
        }
    }
    let doc: Document = typst_to_ir(input);
    if full_document {
        let hints = extract_preamble_hints(input);
        let base_font_size_pt = hints
            .text_size
            .as_deref()
            .and_then(|size| parse_length_to_pt(size, "10pt"));
        let preamble = render_article_preamble(&hints);
        let number_equations = equation_numbering_enabled(&hints);
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
                number_equations,
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
    render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: is_two_column(&hints),
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Bottom,
            bibliography_style_default: hints.bibliography_style.clone(),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    )
}

/// Convert Typst to LaTeX using the IR pipeline and return a loss report.
pub fn typst_to_latex_ir_with_report(input: &str, full_document: bool) -> ConversionReport {
    let doc: Document = typst_to_ir(input);
    let report = build_loss_report(&doc);
    let mut out = typst_to_latex_ir(input, full_document);
    append_loss_markers(&mut out, &report, full_document);
    ConversionReport::new(out, report)
}
