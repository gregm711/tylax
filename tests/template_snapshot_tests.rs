use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tylax::diagnostics::{check_latex, format_diagnostics};
use tylax::typst_to_latex_ir;
use tylax_typst_frontend::typst_to_ir;

fn normalize(s: &str) -> String {
    s.trim().replace("\r\n", "\n")
}

fn collect_templates() -> Vec<(String, PathBuf)> {
    let dir = Path::new("public/templates");
    let mut templates = Vec::new();

    for entry in fs::read_dir(dir).expect("read templates dir") {
        let entry = entry.expect("read entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("typ") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        templates.push((name, path));
    }

    templates.sort_by(|a, b| a.0.cmp(&b.0));
    templates
}

fn snapshot_template(name: &str, input_path: &Path) {
    let input = fs::read_to_string(input_path).expect("template missing");
    let output = typst_to_latex_ir(&input, true);

    if std::env::var("CHECK_LATEX").is_ok() {
        let diagnostics = check_latex(&output);
        if diagnostics.has_errors() {
            let details = format_diagnostics(&diagnostics, false);
            panic!(
                "LaTeX diagnostics reported errors for {}:\n{}",
                name, details
            );
        }
    }

    let out_dir = PathBuf::from("tests/fixtures/templates");
    let expected_path = out_dir.join(format!("{}.tex", name));

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        fs::create_dir_all(&out_dir).expect("create snapshot dir");
        fs::write(&expected_path, output).expect("write snapshot");
        return;
    }

    let expected =
        fs::read_to_string(&expected_path).expect("missing snapshot; set UPDATE_GOLDEN=1");
    assert_eq!(normalize(&output), normalize(&expected));
}

fn write_loss_report(templates: &[(String, PathBuf)]) {
    let mut report = String::from("# Template Loss Report\n\n");
    let mut overall: BTreeMap<String, usize> = BTreeMap::new();

    for (name, path) in templates {
        let input = fs::read_to_string(path).expect("template missing");
        let doc = typst_to_ir(&input);
        let mut per_kind: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();

        for loss in doc.losses {
            let entry = per_kind
                .entry(loss.kind.clone())
                .or_insert_with(|| (0, BTreeSet::new()));
            entry.0 += 1;
            if entry.1.len() < 3 {
                entry.1.insert(loss.message.clone());
            }
            *overall.entry(loss.kind).or_insert(0) += 1;
        }

        report.push_str(&format!("## {}\n", name));
        if per_kind.is_empty() {
            report.push_str("- No losses detected\n\n");
            continue;
        }
        for (kind, (count, samples)) in per_kind {
            let mut line = format!("- {}: {}", kind, count);
            if !samples.is_empty() {
                let examples: Vec<_> = samples.into_iter().collect();
                line.push_str(&format!(" (e.g., {})", examples.join(" | ")));
            }
            report.push_str(&line);
            report.push('\n');
        }
        report.push('\n');
    }

    report.push_str("## Overall\n");
    for (kind, count) in overall {
        report.push_str(&format!("- {}: {}\n", kind, count));
    }
    report.push('\n');

    let out_dir = PathBuf::from("tests/fixtures/templates");
    fs::create_dir_all(&out_dir).expect("create loss report dir");
    fs::write(out_dir.join("losses.md"), report).expect("write loss report");
}

#[test]
fn template_snapshots() {
    let templates = collect_templates();
    if std::env::var("UPDATE_LOSS_REPORT").is_ok() {
        write_loss_report(&templates);
    }
    let only = std::env::var("ONLY_TEMPLATE").ok();

    for (name, path) in templates {
        if let Some(ref only_name) = only {
            if name != *only_name {
                continue;
            }
        }
        snapshot_template(&name, &path);
    }
}

#[test]
fn ieee_adapter_structure() {
    let input =
        std::fs::read_to_string("public/templates/ieee.typ").expect("missing ieee template");
    let output = typst_to_latex_ir(&input, true);
    assert!(
        output.contains("\\documentclass[conference]{IEEEtran}"),
        "IEEE adapter should emit IEEEtran class"
    );
    assert!(
        output.contains("\\title{"),
        "IEEE adapter should emit standard IEEE title command"
    );
    assert!(
        output.contains("\\IEEEauthorblockN"),
        "IEEE adapter should emit IEEE author blocks"
    );
    assert!(
        output.contains("\\maketitle"),
        "IEEE adapter should call maketitle"
    );
    assert!(
        output.contains("\\begin{abstract}"),
        "IEEE adapter should emit abstract environment"
    );
}
