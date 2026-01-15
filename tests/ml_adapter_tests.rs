use std::fs;

use tylax::typst_to_latex_ir;

fn load(path: &str) -> String {
    fs::read_to_string(path).expect("missing typst template")
}

#[test]
fn icml_adapter_emits_style_package() {
    let input = load("typst-corpus/ml-templates/icml/main.typ");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\IfFileExists{icml2025.sty}"),
        "ICML adapter should emit icml2025 package fallback"
    );
    assert!(output.contains("\\maketitle"));
}

#[test]
fn neurips_adapter_emits_style_package() {
    let input = load("typst-corpus/ml-templates/neurips/main.typ");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\IfFileExists{neurips_2025.sty}"),
        "NeurIPS adapter should emit neurips_2025 package fallback"
    );
    assert!(output.contains("\\maketitle"));
}

#[test]
fn iclr_adapter_emits_style_package() {
    let input = load("typst-corpus/ml-templates/iclr/main.typ");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\IfFileExists{iclr2025_conference.sty}"),
        "ICLR adapter should emit iclr2025_conference package fallback"
    );
    assert!(output.contains("\\maketitle"));
}

#[test]
fn cvpr_adapter_emits_style_package() {
    let input = load("typst-corpus/ml-templates/cvpr/main.typ");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\IfFileExists{cvpr.sty}"),
        "CVPR adapter should emit cvpr package fallback"
    );
    assert!(output.contains("\\maketitle"));
}

#[test]
fn tmlr_adapter_emits_style_package() {
    let input = load("typst-corpus/ml-templates/tmlr/main.typ");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\IfFileExists{tmlr.sty}"),
        "TMLR adapter should emit tmlr package fallback"
    );
    assert!(output.contains("\\maketitle"));
}

#[test]
fn jmlr_adapter_emits_style_package() {
    let input = load("typst-corpus/ml-templates/jmlr/main.typ");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\IfFileExists{jmlr2e.sty}"),
        "JMLR adapter should emit jmlr2e package fallback"
    );
    assert!(output.contains("\\maketitle"));
}
