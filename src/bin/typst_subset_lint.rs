use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use tylax::utils::typst_analysis::lint_source;

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
    let warn_only = if let Some(pos) = args
        .iter()
        .position(|a| a == "--warn" || a == "--warn-only")
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
        let local_issues = lint_source(&source);
        for issue in local_issues {
            issues.push(Issue {
                file: file.clone(),
                line: issue.line,
                col: issue.col,
                kind: issue.kind,
                message: issue.message,
            });
        }
    }

    if issues.is_empty() {
        println!("No subset violations found.");
        return;
    }

    for issue in &issues {
        if issue.line == 0 {
            println!(
                "{}: {}: {}",
                issue.file.display(),
                issue.kind,
                issue.message
            );
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
