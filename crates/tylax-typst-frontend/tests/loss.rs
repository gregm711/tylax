use tylax_typst_frontend::typst_to_ir;

#[test]
fn table_style_loss_is_reported() {
    let input = "#table(columns: 2, stroke: 1pt, [A], [B])";
    let doc = typst_to_ir(input);
    assert!(doc.losses.iter().any(|l| l.kind == "table-style"));
}
