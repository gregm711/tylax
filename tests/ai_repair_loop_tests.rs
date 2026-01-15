use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let unique = format!("tylax-{}-{}", name, std::process::id());
    dir.push(unique);
    let _ = fs::create_dir_all(&dir);
    dir
}

fn write_file(path: &Path, content: &str) {
    let mut file = fs::File::create(path).expect("create file");
    file.write_all(content.as_bytes()).expect("write file");
}

fn run_repair(
    input_path: &Path,
    ai_cmd: &str,
    full_document: bool,
) -> String {
    let bin = env!("CARGO_BIN_EXE_tylax_repair");
    let mut cmd = Command::new(bin);
    cmd.arg(input_path);
    if full_document {
        cmd.arg("--full-document");
    }
    cmd.arg("--auto-repair");
    cmd.arg("--ai-cmd").arg(ai_cmd);

    let output = cmd.output().expect("run tylax-repair");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn run_t2l_repair_typst(
    input_path: &Path,
    ai_cmd: &str,
    full_document: bool,
) -> String {
    let bin = env!("CARGO_BIN_EXE_t2l");
    let mut cmd = Command::new(bin);
    cmd.arg(input_path);
    cmd.arg("--direction").arg("t2l");
    cmd.arg("--ir");
    if full_document {
        cmd.arg("--full-document");
    }
    cmd.arg("--auto-repair");
    cmd.arg("--ai-cmd").arg(ai_cmd);

    let output = cmd.output().expect("run t2l");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn auto_repair_accepts_improvement() {
    let dir = temp_dir("repair-accepts");
    let input_path = dir.join("input.tex");
    let script_path = dir.join("ai_fix.sh");

    write_file(
        &input_path,
        r"\unknowncmd{a}",
    );

    write_file(
        &script_path,
        r#"#!/usr/bin/env python3
import json, re, sys
payload = json.load(sys.stdin)
out = payload.get("output", "")
out = re.sub(r"/\*\s*tylax:loss:[^*]*\*/\s*", "", out)
print(out.strip())
"#,
    );

    let ai_cmd = format!("python3 {}", script_path.display());
    let output = run_repair(&input_path, &ai_cmd, false);

    assert!(!output.contains("tylax:loss:"), "loss marker should be removed");
}

#[test]
fn auto_repair_rejects_regression() {
    let dir = temp_dir("repair-rejects");
    let input_path = dir.join("input.tex");
    let script_path = dir.join("ai_bad.py");

    write_file(
        &input_path,
        r"\documentclass{article}
\begin{document}
\section{Intro}
\unknowncmd{a}
\end{document}",
    );

    write_file(
        &script_path,
        r#"#!/usr/bin/env python3
import sys
print("")
"#,
    );

    let ai_cmd = format!("python3 {}", script_path.display());
    let output = run_repair(&input_path, &ai_cmd, true);

    assert!(output.contains("== Intro"));
    assert!(output.contains("tylax:loss:"));
}

#[test]
fn auto_repair_typst_to_latex_accepts_improvement() {
    let dir = temp_dir("repair-accepts-t2l");
    let input_path = dir.join("input.typ");
    let script_path = dir.join("ai_fix.py");

    write_file(
        &input_path,
        r#"#outline(target: "foo")"#,
    );

    write_file(
        &script_path,
        r#"#!/bin/sh
cat <<'EOF'
\tableofcontents
EOF
"#,
    );

    let ai_cmd = format!("sh {}", script_path.display());
    let output = run_t2l_repair_typst(&input_path, &ai_cmd, false);

    assert!(!output.contains("tylax:loss:"), "loss marker should be removed");
}

#[test]
fn auto_repair_typst_to_latex_rejects_regression() {
    let dir = temp_dir("repair-rejects-t2l");
    let input_path = dir.join("input.typ");
    let script_path = dir.join("ai_bad.sh");

    write_file(
        &input_path,
        "= Intro\n\n#outline(target: \"foo\")",
    );

    write_file(
        &script_path,
        r#"#!/bin/sh
exit 0
"#,
    );

    let ai_cmd = format!("sh {}", script_path.display());
    let output = run_t2l_repair_typst(&input_path, &ai_cmd, true);

    assert!(output.contains("\\section"));
    assert!(output.contains("tylax:loss:"));
}
