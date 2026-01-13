use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use typst_syntax::{parse, SyntaxKind, SyntaxNode};

#[derive(Debug)]
struct Issue {
    file: PathBuf,
    line: usize,
    col: usize,
    kind: &'static str,
    message: String,
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let warn_only = if let Some(pos) = args.iter().position(|a| a == "--warn" || a == "--warn-only")
    {
        args.remove(pos);
        true
    } else {
        false
    };

    let paths: Vec<PathBuf> = if args.is_empty() {
        vec![PathBuf::from("../public/templates")]
    } else {
        args.iter().map(PathBuf::from).collect()
    };

    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            collect_typ_files(&path, &mut files);
        } else if path.extension().and_then(|s| s.to_str()) == Some("typ") {
            files.push(path);
        }
    }

    if files.is_empty() {
        eprintln!("No .typ files found.");
        std::process::exit(1);
    }

    let mut issues = Vec::new();
    for file in files {
        let Ok(source) = fs::read_to_string(&file) else {
            issues.push(Issue {
                file: file.clone(),
                line: 0,
                col: 0,
                kind: "io",
                message: "Failed to read file".to_string(),
            });
            continue;
        };
        let root = parse(&source);
        lint_node(&root, &file, &mut issues);
    }

    if issues.is_empty() {
        println!("No subset violations found.");
        return;
    }

    for issue in &issues {
        if issue.line == 0 {
            println!("{}: {}: {}", issue.file.display(), issue.kind, issue.message);
        } else {
            println!(
                "{}:{}:{}: {}: {}",
                issue.file.display(),
                issue.line,
                issue.col,
                issue.kind,
                issue.message
            );
        }
    }

    if !warn_only {
        std::process::exit(1);
    }
}

fn collect_typ_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_typ_files(&path, files);
        } else if path.extension().and_then(|s| s.to_str()) == Some("typ") {
            files.push(path);
        }
    }
}

fn lint_node(node: &SyntaxNode, file: &Path, issues: &mut Vec<Issue>) {
    match node.kind() {
        SyntaxKind::Dots => {
            push_issue(file, issues, "error", "spread/ellipsis '..' is not allowed");
        }
        SyntaxKind::ShowRule => {
            if !is_allowed_show_rule(node) {
                push_issue(file, issues, "error", "show rules are not allowed");
            }
        }
        SyntaxKind::SetRule => {
            if let Some(name) = set_rule_name(node) {
                if matches!(
                    name.as_str(),
                    "page" | "text" | "par" | "math.equation" | "std.bibliography"
                ) {
                    // Allowed: subset supports these set rules for preamble hints.
                } else {
                    push_issue(file, issues, "error", "set rules are not allowed");
                }
            } else {
                push_issue(file, issues, "error", "set rules are not allowed");
            }
        }
        SyntaxKind::CodeBlock => {
            push_issue(file, issues, "error", "code blocks { ... } are not allowed; use [ ... ] content blocks");
        }
        SyntaxKind::LetBinding => {
            if node.children().any(|c| c.kind() == SyntaxKind::CodeBlock) {
                push_issue(file, issues, "error", "#let with code block body is not allowed");
            }
        }
        SyntaxKind::Binary => {
            if node.children().any(|c| c.kind() == SyntaxKind::In) {
                push_issue(file, issues, "error", "the `in` operator is not allowed (use explicit fields or booleans)");
            }
        }
        SyntaxKind::FuncCall => {
            if let Some(name) = func_call_name(node) {
                if name == "place" {
                    push_issue(file, issues, "error", "place(...) is not allowed; use align/block/box");
                }
                if name.starts_with("calc.") || name == "calc" {
                    push_issue(file, issues, "error", "calc.* is not allowed");
                }
                if let Some(method) = name.split('.').last() {
                    if matches!(method, "map" | "filter" | "fold" | "reduce" | "join") {
                        push_issue(file, issues, "error", "functional collection methods are not allowed; use #for loops");
                    }
                }
            }
        }
        _ => {}
    }

    for child in node.children() {
        lint_node(&child, file, issues);
    }
}

fn set_rule_name(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Ident => return Some(child.text().to_string()),
            SyntaxKind::FieldAccess => {
                let mut parts = Vec::new();
                for part in child.children() {
                    if part.kind() == SyntaxKind::Ident {
                        parts.push(part.text().to_string());
                    }
                }
                if !parts.is_empty() {
                    return Some(parts.join("."));
                }
            }
            _ => {}
        }
    }
    None
}

fn is_allowed_show_rule(node: &SyntaxNode) -> bool {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Ident => {
                if child.text() == "heading" {
                    return true;
                }
            }
            SyntaxKind::FuncCall => {
                if let Some(name) = func_call_name(&child) {
                    if name == "heading.where" || name == "heading" {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn func_call_name(node: &SyntaxNode) -> Option<String> {
    let first = node.children().next()?;
    if first.kind() == SyntaxKind::Ident {
        return Some(first.text().to_string());
    }
    if first.kind() == SyntaxKind::FieldAccess {
        let mut parts = Vec::new();
        for child in first.children() {
            if child.kind() == SyntaxKind::Ident {
                parts.push(child.text().to_string());
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("."));
        }
    }
    None
}

fn push_issue(file: &Path, issues: &mut Vec<Issue>, kind: &'static str, message: &str) {
    issues.push(Issue {
        file: file.to_path_buf(),
        line: 0,
        col: 0,
        kind,
        message: message.to_string(),
    });
}
