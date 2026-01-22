//! LaTeX analysis utilities: basic metrics and linting.

use serde::Serialize;

use crate::utils::diagnostics::check_latex;

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct LatexMetrics {
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
    pub warnings: usize,
}

impl LatexMetrics {
    pub fn at_least(&self, baseline: &LatexMetrics) -> bool {
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

pub fn lint_source(source: &str) -> (usize, usize) {
    let result = check_latex(source);
    (result.errors, result.warnings)
}

pub fn metrics_source(source: &str, loss_marker_prefix: &str) -> LatexMetrics {
    let mut metrics = LatexMetrics::default();

    metrics.loss_markers = source.matches(loss_marker_prefix).count();

    metrics.headings = count_any(source, &["\\section{", "\\subsection{", "\\subsubsection{"]);
    metrics.equations = count_any(
        source,
        &[
            "\\begin{equation}",
            "\\begin{align}",
            "\\begin{eqnarray}",
            "\\[",
        ],
    );
    metrics.figures = count_any(source, &["\\begin{figure}"]);
    metrics.tables = count_any(source, &["\\begin{table}"]);
    metrics.cites = count_regex_like(source, "\\\\cite");
    metrics.refs = count_any(source, &["\\ref{", "\\eqref{"]);
    metrics.labels = count_any(source, &["\\label{"]);
    metrics.list_items = count_any(source, &["\\item ", "\\item\n"]);

    let (errors, warnings) = lint_source(source);
    metrics.parse_errors = errors;
    metrics.warnings = warnings;

    metrics
}

fn count_any(haystack: &str, needles: &[&str]) -> usize {
    needles.iter().map(|n| haystack.matches(n).count()).sum()
}

fn count_regex_like(haystack: &str, needle_prefix: &str) -> usize {
    // Simple prefix-based count for \cite variants
    haystack.matches(needle_prefix).count()
}
