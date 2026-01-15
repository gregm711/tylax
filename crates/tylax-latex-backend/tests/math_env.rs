use tylax_ir::{Block, Document, MathBlock};
use tylax_latex_backend::{render_document, LatexRenderOptions};

#[test]
fn math_block_uses_gather_without_alignment() {
    let doc = Document::new(vec![Block::MathBlock(MathBlock {
        content: "a \\\\ b".to_string(),
        label: None,
    })]);
    let out = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: true,
            two_column: false,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Bottom,
            bibliography_style_default: None,
            cite_command: None,
            base_font_size_pt: None,
        },
    );
    assert!(out.contains("\\begin{gather}"));
    assert!(out.contains("\\end{gather}"));
}

#[test]
fn math_block_uses_align_with_alignment_points() {
    let doc = Document::new(vec![Block::MathBlock(MathBlock {
        content: "a &= b \\\\ c &= d".to_string(),
        label: None,
    })]);
    let out = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: true,
            two_column: false,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Bottom,
            bibliography_style_default: None,
            cite_command: None,
            base_font_size_pt: None,
        },
    );
    assert!(out.contains("\\begin{align}"));
    assert!(out.contains("\\end{align}"));
}
