//! Utility modules
//!
//! This module contains utilities and helpers:
//! - Diagnostics and error reporting
//! - File resolution for multi-file documents
//! - Error types and result types

pub mod diagnostics;
pub mod error;
pub mod files;
pub mod latex_analysis;
pub mod loss;
pub mod repair;
pub mod typst_analysis;

// Re-export commonly used items
pub use diagnostics::{check_latex, format_diagnostics, Diagnostic, DiagnosticLevel};
pub use error::{ConversionError, ConversionOutput, ConversionResult, ConversionWarning};
pub use files::{FileResolveError, FileResolver, MemoryFileResolver, NoopFileResolver};
pub use latex_analysis::{lint_source as lint_latex_source, LatexMetrics};
pub use loss::{ConversionReport, LossKind, LossRecord, LossReport, LOSS_MARKER_PREFIX};
pub use repair::AiRepairConfig;
pub use typst_analysis::{lint_source as lint_typst_source, TypstIssue, TypstMetrics};

#[cfg(not(target_arch = "wasm32"))]
pub use files::StdFileResolver;
