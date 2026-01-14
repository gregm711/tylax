use std::fs;
use std::path::Path;

use tylax::typst_to_latex_ir;

fn read_fixture(path: &str) -> String {
    fs::read_to_string(Path::new(path)).expect("fixture missing")
}

fn normalize(s: &str) -> String {
    s.trim().replace("\r\n", "\n")
}

#[test]
fn ir_pipeline_simple() {
    let input = read_fixture("tests/fixtures/typst/simple.typ");
    let expected = read_fixture("tests/fixtures/latex/simple.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_list() {
    let input = read_fixture("tests/fixtures/typst/list.typ");
    let expected = read_fixture("tests/fixtures/latex/list.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_block() {
    let input = read_fixture("tests/fixtures/typst/math.typ");
    let expected = read_fixture("tests/fixtures/latex/math.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_numbering() {
    let input = read_fixture("tests/fixtures/typst/math-numbering.typ");
    let expected = read_fixture("tests/fixtures/latex/math-numbering.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_label() {
    let input = read_fixture("tests/fixtures/typst/math-label.typ");
    let expected = read_fixture("tests/fixtures/latex/math-label.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_cases() {
    let input = read_fixture("tests/fixtures/typst/math-cases.typ");
    let expected = read_fixture("tests/fixtures/latex/math-cases.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_mat() {
    let input = read_fixture("tests/fixtures/typst/math-mat.typ");
    let expected = read_fixture("tests/fixtures/latex/math-mat.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_funcs() {
    let input = read_fixture("tests/fixtures/typst/math-funcs.typ");
    let expected = read_fixture("tests/fixtures/latex/math-funcs.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_operators() {
    let input = read_fixture("tests/fixtures/typst/math-operators.typ");
    let expected = read_fixture("tests/fixtures/latex/math-operators.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_math_symbols() {
    let input = read_fixture("tests/fixtures/typst/math-symbols.typ");
    let expected = read_fixture("tests/fixtures/latex/math-symbols.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_table() {
    let input = read_fixture("tests/fixtures/typst/table.typ");
    let expected = read_fixture("tests/fixtures/latex/table.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_table_label() {
    let input = read_fixture("tests/fixtures/typst/table-label.typ");
    let expected = read_fixture("tests/fixtures/latex/table-label.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_table_header() {
    let input = read_fixture("tests/fixtures/typst/table-header.typ");
    let expected = read_fixture("tests/fixtures/latex/table-header.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_figure() {
    let input = read_fixture("tests/fixtures/typst/figure.typ");
    let expected = read_fixture("tests/fixtures/latex/figure.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_heading_func() {
    let input = read_fixture("tests/fixtures/typst/heading-func.typ");
    let expected = read_fixture("tests/fixtures/latex/heading-func.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_heading_label() {
    let input = read_fixture("tests/fixtures/typst/heading-label.typ");
    let expected = read_fixture("tests/fixtures/latex/heading-label.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_references_block() {
    let input = read_fixture("tests/fixtures/typst/references.typ");
    let expected = read_fixture("tests/fixtures/latex/references.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_theorem_env() {
    let input = read_fixture("tests/fixtures/typst/theorem.typ");
    let expected = read_fixture("tests/fixtures/latex/theorem.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_theorem_label() {
    let input = read_fixture("tests/fixtures/typst/theorem-label.typ");
    let expected = read_fixture("tests/fixtures/latex/theorem-label.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_theorem_preamble() {
    let input = "#theorem[Sample theorem.]";
    let output = typst_to_latex_ir(input, true);
    assert!(output.contains("\\usepackage{amsthm}"));
    assert!(output.contains("\\newtheorem{theorem}"));
    assert!(output.contains("\\begin{theorem}"));
}

#[test]
fn ir_pipeline_figure_placement_equation() {
    let input = read_fixture("tests/fixtures/typst/figure-placement-equation.typ");
    let expected = read_fixture("tests/fixtures/latex/figure-placement-equation.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_figure_two_column() {
    let input = read_fixture("tests/fixtures/typst/figure-two-column.typ");
    let expected = read_fixture("tests/fixtures/latex/figure-two-column.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_image() {
    let input = read_fixture("tests/fixtures/typst/image.typ");
    let expected = read_fixture("tests/fixtures/latex/image.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_refs() {
    let input = read_fixture("tests/fixtures/typst/refs.typ");
    let expected = read_fixture("tests/fixtures/latex/refs.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_cite() {
    let input = read_fixture("tests/fixtures/typst/cite.typ");
    let expected = read_fixture("tests/fixtures/latex/cite.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_cite_multi() {
    let input = read_fixture("tests/fixtures/typst/cite-multi.typ");
    let expected = read_fixture("tests/fixtures/latex/cite-multi.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_cite_array() {
    let input = read_fixture("tests/fixtures/typst/cite-array.typ");
    let expected = read_fixture("tests/fixtures/latex/cite-array.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_footnote() {
    let input = read_fixture("tests/fixtures/typst/footnote.typ");
    let expected = read_fixture("tests/fixtures/latex/footnote.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_bibliography() {
    let input = read_fixture("tests/fixtures/typst/bibliography.typ");
    let expected = read_fixture("tests/fixtures/latex/bibliography.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_bibliography_style() {
    let input = read_fixture("tests/fixtures/typst/bibliography-style.typ");
    let expected = read_fixture("tests/fixtures/latex/bibliography-style.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_bibliography_default_style() {
    let input = read_fixture("tests/fixtures/typst/bibliography-default-style.typ");
    let expected = read_fixture("tests/fixtures/latex/bibliography-default-style.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_bibliography_multi() {
    let input = read_fixture("tests/fixtures/typst/bibliography-multi.typ");
    let expected = read_fixture("tests/fixtures/latex/bibliography-multi.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_outline() {
    let input = read_fixture("tests/fixtures/typst/outline.typ");
    let expected = read_fixture("tests/fixtures/latex/outline.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_inline_color() {
    let input = read_fixture("tests/fixtures/typst/inline-color.typ");
    let expected = read_fixture("tests/fixtures/latex/inline-color.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_super_sub() {
    let input = read_fixture("tests/fixtures/typst/super-sub.typ");
    let expected = read_fixture("tests/fixtures/latex/super-sub.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_sym() {
    let input = read_fixture("tests/fixtures/typst/sym.typ");
    let expected = read_fixture("tests/fixtures/latex/sym.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_preamble_hints() {
    let input = r##"
#let primary = rgb("#1e40af")
#set page(paper: "us-letter", margin: (x: 1in, y: 2in))
#set text(font: "New Computer Modern", size: 11pt)
#set par(justify: false, leading: 0.55em, first-line-indent: 1.5em)

Hello world.
"##;
    let output = typst_to_latex_ir(input, true);
    assert!(
        output.contains("\\documentclass[11pt,letterpaper]{article}")
            || output.contains("\\documentclass[letterpaper,11pt]{article}")
    );
    assert!(output.contains("\\usepackage[left=1in,right=1in,top=2in,bottom=2in]{geometry}"));
    assert!(output.contains("\\setlength{\\parindent}{1.5em}"));
    assert!(output.contains("\\definecolor{primary}{HTML}{1E40AF}"));
    assert!(output.contains("\\raggedright"));
}

#[test]
fn ir_pipeline_equation_numberwithin() {
    let input = r##"
#set math.equation(numbering: "(1.1)")

= Section

$ a = b $
"##;
    let output = typst_to_latex_ir(input, true);
    assert!(output.contains("\\numberwithin{equation}{section}"));
}

#[test]
fn ir_pipeline_letter_adapter() {
    let input = r##"
#import "@preview/appreciated-letter:0.1.0": letter
#show: letter.with(
  sender: [Jane Smith],
  recipient: [John Doe],
  date: [June 9th, 2023],
  subject: [Test Subject],
  name: [Jane Smith]
)

Dear John,

Hello there.
"##;
    let output = typst_to_latex_ir(input, true);
    assert!(output.contains("\\documentclass{letter}"));
    assert!(output.contains("\\address{Jane Smith}"));
    assert!(output.contains("\\signature{Jane Smith}"));
    assert!(output.contains("\\begin{letter}{John Doe}"));
    assert!(output.contains("Subject: Test Subject"));
}

#[test]
fn ir_pipeline_book_adapter() {
    let input = r##"
#import "@preview/wonderous-book:0.1.2": book
#show: book.with(
  title: [My Book],
  author: "Jane Doe",
  dedication: [For Rachel],
  publishing-info: [Publisher Name]
)

= Chapter
Hello world.
"##;
    let output = typst_to_latex_ir(input, true);
    assert!(output.contains("\\documentclass{book}"));
    assert!(output.contains("\\title{My Book}"));
    assert!(output.contains("\\author{Jane Doe}"));
    assert!(output.contains("\\frontmatter"));
    assert!(output.contains("\\mainmatter"));
}

#[test]
fn ir_pipeline_ams_adapter() {
    let input = r##"
#import "@preview/unequivocal-ams:0.1.2": ams-article
#show: ams-article.with(
  title: [Math Paper],
  authors: (
    (
      name: "Jane Doe",
      department: [Dept],
      organization: [Uni],
      location: [City],
      email: "jane@example.com",
      url: "example.com"
    ),
  ),
  abstract: [Abstract text],
  bibliography: bibliography("refs.bib"),
)

Hello.
"##;
    let output = typst_to_latex_ir(input, true);
    assert!(output.contains("\\documentclass{amsart}"));
    assert!(output.contains("\\author{Jane Doe}"));
    assert!(output.contains("\\begin{abstract}"));
    assert!(output.contains("\\bibliography{refs}"));
}

#[test]
fn ir_pipeline_newsletter_adapter() {
    let input = r##"
#import "@preview/dashing-dept-news:0.1.1": newsletter
#show: newsletter.with(
  title: [Chemistry Department],
  edition: [March 18th, 2023],
  hero-image: (
    image: image("cover.jpg"),
    caption: [Award-winning science],
  ),
  publication-info: [Some address]
)

= Headline
Hello.
"##;
    let output = typst_to_latex_ir(input, true);
    assert!(output.contains("\\documentclass{article}"));
    assert!(output.contains("\\includegraphics"));
    assert!(output.contains("cover.jpg"));
}

#[test]
fn ir_pipeline_quote() {
    let input = read_fixture("tests/fixtures/typst/quote.typ");
    let expected = read_fixture("tests/fixtures/latex/quote.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_code() {
    let input = read_fixture("tests/fixtures/typst/code.typ");
    let expected = read_fixture("tests/fixtures/latex/code.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_table_span() {
    let input = read_fixture("tests/fixtures/typst/table-span.typ");
    let expected = read_fixture("tests/fixtures/latex/table-span.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_table_rowspan() {
    let input = read_fixture("tests/fixtures/typst/table-rowspan.typ");
    let expected = read_fixture("tests/fixtures/latex/table-rowspan.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_box() {
    let input = read_fixture("tests/fixtures/typst/box.typ");
    let expected = read_fixture("tests/fixtures/latex/box.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_block() {
    let input = read_fixture("tests/fixtures/typst/block.typ");
    let expected = read_fixture("tests/fixtures/latex/block.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_columns() {
    let input = read_fixture("tests/fixtures/typst/columns.typ");
    let expected = read_fixture("tests/fixtures/latex/columns.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_grid() {
    let input = read_fixture("tests/fixtures/typst/grid.typ");
    let expected = read_fixture("tests/fixtures/latex/grid.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_preprocess_basic() {
    let input = read_fixture("tests/fixtures/typst/preprocess-basic.typ");
    let expected = read_fixture("tests/fixtures/latex/preprocess-basic.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_preprocess_counter() {
    let input = read_fixture("tests/fixtures/typst/preprocess-counter.typ");
    let expected = read_fixture("tests/fixtures/latex/preprocess-counter.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}

#[test]
fn ir_pipeline_preprocess_logic() {
    let input = read_fixture("tests/fixtures/typst/preprocess-logic.typ");
    let expected = read_fixture("tests/fixtures/latex/preprocess-logic.tex");
    let output = typst_to_latex_ir(&input, false);
    assert_eq!(normalize(&output), normalize(&expected));
}
