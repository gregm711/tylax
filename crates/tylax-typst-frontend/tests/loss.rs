use tylax_typst_frontend::typst_to_ir;

#[test]
fn table_style_is_supported() {
    let input = "#table(columns: 2, stroke: 1pt, fill: \"#f2f2f2\", inset: 4pt, [A], [B])";
    let doc = typst_to_ir(input);
    assert!(doc.losses.is_empty());
}

#[test]
fn show_rule_heading_is_supported() {
    let input = "#show heading: it => it";
    let doc = typst_to_ir(input);
    assert!(!doc.losses.iter().any(|l| l.kind == "show-rule"));
}

#[test]
fn show_rule_non_heading_is_supported() {
    let input = "#show figure: strong";
    let doc = typst_to_ir(input);
    assert!(!doc.losses.iter().any(|l| l.kind == "show-rule"));
}

#[test]
fn set_rule_page_is_supported() {
    let input = "#set page(margin: 1cm)";
    let doc = typst_to_ir(input);
    assert!(!doc.losses.iter().any(|l| l.kind == "set-rule"));
}

#[test]
fn set_rule_unsupported_reports_loss() {
    let input = "#set figure(caption: none)";
    let doc = typst_to_ir(input);
    assert!(doc.losses.iter().any(|l| l.kind == "set-rule"));
}
