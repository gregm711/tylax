//! AI repair helpers for conversions.

use std::io::{self, Write};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::utils::latex_analysis::{metrics_source as latex_metrics_source, LatexMetrics};
use crate::utils::loss::{LossReport, LOSS_MARKER_PREFIX};
use crate::utils::typst_analysis::{
    lint_source as lint_typst_source, metrics_source as typst_metrics_source, TypstMetrics,
};

#[derive(Debug, Clone)]
pub struct AiRepairConfig {
    pub auto_repair: bool,
    pub ai_cmd: Option<String>,
    pub allow_no_gain: bool,
}

impl AiRepairConfig {
    pub fn from_env() -> Self {
        Self {
            auto_repair: false,
            ai_cmd: std::env::var("TYLAX_AI_CMD").ok(),
            allow_no_gain: false,
        }
    }

    pub fn effective_ai_cmd(&self) -> Option<String> {
        self.ai_cmd
            .clone()
            .or_else(|| std::env::var("TYLAX_AI_CMD").ok())
    }
}

#[derive(Serialize)]
struct AiRepairRequestTypst {
    input: String,
    output: String,
    report: LossReport,
    metrics: TypstMetrics,
}

#[derive(Serialize)]
struct AiRepairRequestLatex {
    input: String,
    output: String,
    report: LossReport,
    metrics: LatexMetrics,
}

pub fn maybe_repair_latex_to_typst(
    input: &str,
    output: &str,
    report: &LossReport,
    config: &AiRepairConfig,
) -> String {
    if !config.auto_repair || report.losses.is_empty() {
        return output.to_string();
    }

    let Some(ai_cmd) = config.effective_ai_cmd() else {
        return output.to_string();
    };

    let repaired = match run_ai_repair_typst(&ai_cmd, input, output, report) {
        Ok(result) => result,
        Err(_) => return output.to_string(),
    };

    if validate_repair_typst(output, &repaired, config.allow_no_gain) {
        repaired
    } else {
        output.to_string()
    }
}

pub fn maybe_repair_typst_to_latex(
    input: &str,
    output: &str,
    report: &LossReport,
    config: &AiRepairConfig,
) -> String {
    if !config.auto_repair || report.losses.is_empty() {
        return output.to_string();
    }

    let Some(ai_cmd) = config.effective_ai_cmd() else {
        return output.to_string();
    };

    let repaired = match run_ai_repair_latex(&ai_cmd, input, output, report) {
        Ok(result) => result,
        Err(_) => return output.to_string(),
    };

    if validate_repair_latex(output, &repaired, config.allow_no_gain) {
        repaired
    } else {
        output.to_string()
    }
}

fn run_ai_repair_typst(
    cmd: &str,
    input: &str,
    output: &str,
    report: &LossReport,
) -> io::Result<String> {
    let metrics = typst_metrics_source(output, LOSS_MARKER_PREFIX);
    let payload = AiRepairRequestTypst {
        input: input.to_string(),
        output: output.to_string(),
        report: report.clone(),
        metrics,
    };

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        let serialized = serde_json::to_string(&payload)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        stdin.write_all(serialized.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "AI repair command failed",
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_ai_repair_latex(
    cmd: &str,
    input: &str,
    output: &str,
    report: &LossReport,
) -> io::Result<String> {
    let metrics = latex_metrics_source(output, LOSS_MARKER_PREFIX);
    let payload = AiRepairRequestLatex {
        input: input.to_string(),
        output: output.to_string(),
        report: report.clone(),
        metrics,
    };

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        let serialized = serde_json::to_string(&payload)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        stdin.write_all(serialized.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "AI repair command failed",
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn validate_repair_typst(base: &str, candidate: &str, allow_no_gain: bool) -> bool {
    let base_metrics = typst_metrics_source(base, LOSS_MARKER_PREFIX);
    let candidate_metrics = typst_metrics_source(candidate, LOSS_MARKER_PREFIX);

    if candidate_metrics.parse_errors > 0 {
        return false;
    }

    let base_issues = lint_typst_source(base).len();
    let candidate_issues = lint_typst_source(candidate).len();
    if candidate_issues > base_issues {
        return false;
    }

    if !candidate_metrics.at_least(&base_metrics) {
        return false;
    }

    if !allow_no_gain && base_metrics.loss_markers > 0 {
        if candidate_metrics.loss_markers >= base_metrics.loss_markers {
            return false;
        }
    }

    true
}

fn validate_repair_latex(base: &str, candidate: &str, allow_no_gain: bool) -> bool {
    let base_metrics = latex_metrics_source(base, LOSS_MARKER_PREFIX);
    let candidate_metrics = latex_metrics_source(candidate, LOSS_MARKER_PREFIX);

    if candidate_metrics.parse_errors > base_metrics.parse_errors {
        return false;
    }

    if candidate_metrics.warnings > base_metrics.warnings {
        return false;
    }

    if !candidate_metrics.at_least(&base_metrics) {
        return false;
    }

    if !allow_no_gain && base_metrics.loss_markers > 0 {
        if candidate_metrics.loss_markers >= base_metrics.loss_markers {
            return false;
        }
    }

    true
}
