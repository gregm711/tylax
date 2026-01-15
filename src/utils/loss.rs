//! Loss reporting for non-deterministic or unsupported conversions.

use serde::Serialize;
use tylax_ir::Loss as IrLoss;

pub const LOSS_MARKER_PREFIX: &str = "tylax:loss:";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LossKind {
    UnknownCommand,
    UnknownEnvironment,
    ParseError,
    UnsupportedFeature,
    MacroExpansion,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct LossRecord {
    pub id: String,
    pub kind: LossKind,
    pub name: Option<String>,
    pub message: String,
    pub snippet: Option<String>,
    pub context: Option<String>,
}

impl LossRecord {
    pub fn new(
        id: String,
        kind: LossKind,
        name: Option<String>,
        message: impl Into<String>,
        snippet: Option<String>,
        context: Option<String>,
    ) -> Self {
        Self {
            id,
            kind,
            name,
            message: message.into(),
            snippet,
            context,
        }
    }

    pub fn from_ir_loss(id: String, loss: &IrLoss) -> Self {
        Self {
            id,
            kind: LossKind::Other,
            name: Some(loss.kind.clone()),
            message: loss.message.clone(),
            snippet: None,
            context: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LossReport {
    pub source_lang: String,
    pub target_lang: String,
    pub losses: Vec<LossRecord>,
    pub warnings: Vec<String>,
}

impl LossReport {
    pub fn new(
        source_lang: impl Into<String>,
        target_lang: impl Into<String>,
        losses: Vec<LossRecord>,
        warnings: Vec<String>,
    ) -> Self {
        Self {
            source_lang: source_lang.into(),
            target_lang: target_lang.into(),
            losses,
            warnings,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.losses.is_empty() && self.warnings.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConversionReport {
    pub content: String,
    pub report: LossReport,
}

impl ConversionReport {
    pub fn new(content: String, report: LossReport) -> Self {
        Self { content, report }
    }
}
