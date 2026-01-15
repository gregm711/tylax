//! Typst analysis utilities: subset linting and metrics.

use serde::Serialize;
use typst_syntax::{parse, SyntaxKind, SyntaxNode};

#[derive(Debug, Clone)]
pub struct TypstIssue {
    pub line: usize,
    pub col: usize,
    pub kind: &'static str,
    pub message: String,
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct TypstMetrics {
    pub headings: usize,
    pub equations: usize,
    pub figures: usize,
    pub tables: usize,
    pub cites: usize,
    pub refs: usize,
    pub labels: usize,
    pub list_items: usize,
    pub loss_markers: usize,
    pub parse_errors: usize,
}

impl TypstMetrics {
    pub fn at_least(&self, baseline: &TypstMetrics) -> bool {
        self.headings >= baseline.headings
            && self.equations >= baseline.equations
            && self.figures >= baseline.figures
            && self.tables >= baseline.tables
            && self.cites >= baseline.cites
            && self.refs >= baseline.refs
            && self.labels >= baseline.labels
            && self.list_items >= baseline.list_items
    }
}

pub fn lint_source(source: &str) -> Vec<TypstIssue> {
    let root = parse(source);
    let mut issues = Vec::new();
    lint_node(&root, &mut issues);
    issues
}

pub fn metrics_source(source: &str, loss_marker_prefix: &str) -> TypstMetrics {
    let root = parse(source);
    let mut metrics = TypstMetrics::default();
    metrics.loss_markers = source.matches(loss_marker_prefix).count();
    metrics_node(&root, &mut metrics);
    metrics
}

fn lint_node(node: &SyntaxNode, issues: &mut Vec<TypstIssue>) {
    match node.kind() {
        SyntaxKind::Dots => {
            push_issue(issues, "error", "spread/ellipsis '..' is not allowed");
        }
        SyntaxKind::ShowRule => {
            if !is_allowed_show_rule(node) {
                push_issue(issues, "error", "show rules are not allowed");
            }
        }
        SyntaxKind::SetRule => {
            if let Some(name) = set_rule_name(node) {
                if matches!(
                    name.as_str(),
                    "page" | "text" | "par" | "math.equation" | "std.bibliography"
                ) {
                    // Allowed
                } else {
                    push_issue(issues, "error", "set rules are not allowed");
                }
            } else {
                push_issue(issues, "error", "set rules are not allowed");
            }
        }
        SyntaxKind::CodeBlock => {
            push_issue(
                issues,
                "error",
                "code blocks { ... } are not allowed; use [ ... ] content blocks",
            );
        }
        SyntaxKind::LetBinding => {
            if node.children().any(|c| c.kind() == SyntaxKind::CodeBlock) {
                push_issue(issues, "error", "#let with code block body is not allowed");
            }
        }
        SyntaxKind::Binary => {
            if node.children().any(|c| c.kind() == SyntaxKind::In) {
                push_issue(
                    issues,
                    "error",
                    "the `in` operator is not allowed (use explicit fields or booleans)",
                );
            }
        }
        SyntaxKind::FuncCall => {
            if let Some(name) = func_call_name(node) {
                if name == "place" {
                    push_issue(issues, "error", "place(...) is not allowed; use align/block/box");
                }
                if name.starts_with("calc.") || name == "calc" {
                    push_issue(issues, "error", "calc.* is not allowed");
                }
                if let Some(method) = name.split('.').last() {
                    if matches!(method, "map" | "filter" | "fold" | "reduce" | "join") {
                        push_issue(
                            issues,
                            "error",
                            "functional collection methods are not allowed; use #for loops",
                        );
                    }
                }
            }
        }
        SyntaxKind::Error => {
            push_issue(issues, "error", "parse error");
        }
        _ => {}
    }

    for child in node.children() {
        lint_node(&child, issues);
    }
}

fn metrics_node(node: &SyntaxNode, metrics: &mut TypstMetrics) {
    match node.kind() {
        SyntaxKind::Heading => metrics.headings += 1,
        SyntaxKind::Equation => metrics.equations += 1,
        SyntaxKind::ListItem | SyntaxKind::EnumItem => metrics.list_items += 1,
        SyntaxKind::Label => metrics.labels += 1,
        SyntaxKind::FuncCall => {
            if let Some(name) = func_call_name(node) {
                match name.as_str() {
                    "figure" => metrics.figures += 1,
                    "table" => metrics.tables += 1,
                    "cite" | "bibliography" => metrics.cites += 1,
                    "ref" => metrics.refs += 1,
                    _ => {}
                }
            }
        }
        SyntaxKind::Error => metrics.parse_errors += 1,
        _ => {}
    }

    for child in node.children() {
        metrics_node(&child, metrics);
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

fn push_issue(issues: &mut Vec<TypstIssue>, kind: &'static str, message: &str) {
    issues.push(TypstIssue {
        line: 0,
        col: 0,
        kind,
        message: message.to_string(),
    });
}
