//! Core state and structures for LaTeX to Typst conversion
//!
//! This module contains the main converter struct and conversion state.

use mitex_parser::syntax::{CmdItem, EnvItem, SyntaxElement, SyntaxKind, SyntaxNode};
use mitex_parser::CommandSpec;
use mitex_spec_gen::DEFAULT_SPEC;
use rowan::ast::AstNode;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::time::Instant;

use crate::data::constants::{AcronymDef, GlossaryDef};
use crate::data::colors::{parse_color_with_model, sanitize_color_expression, sanitize_color_identifier};
use crate::data::maps::TEX_COMMAND_SPEC;
use crate::features::templates::{
    generate_title_block, generate_typst_preamble, parse_document_class, DocumentClass,
};
use crate::utils::loss::{LossKind, LossRecord, LossReport};
use fxhash::FxHashMap;
use lazy_static::lazy_static;

use super::engine::lexer::{detokenize, tokenize};
use super::engine::primitives::{parse_definitions, DefinitionKind};
use super::engine::{ArgumentErrorType, EngineWarning};
use super::{ConversionResult, ConversionWarning, WarningKind};

use super::utils::{
    attach_orphan_labels, clean_whitespace, convert_caption_text, escape_typst_string,
    extract_arg_content, extract_arg_content_with_braces, extract_curly_inner_content,
    protect_zero_arg_commands, replace_verb_commands, resolve_reference_markers,
    restore_protected_commands, strip_latex_comments,
};

struct ElementProfileGuard {
    label: String,
    env: Option<String>,
    start: Instant,
}

impl ElementProfileGuard {
    fn new(label: String, env: Option<String>) -> Self {
        ElementProfileGuard {
            label,
            env,
            start: Instant::now(),
        }
    }
}

impl Drop for ElementProfileGuard {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_secs_f64();
        if elapsed >= 0.01 {
            let env_suffix = self
                .env
                .as_ref()
                .map(|env| format!(", env: {}", env))
                .unwrap_or_default();
            eprintln!(
                "[tylax] slow elem {}{env_suffix} {:.3}s",
                self.label, elapsed
            );
        }
    }
}

// =============================================================================
// LaTeX → Typst Conversion Options
// =============================================================================

/// Options for LaTeX to Typst conversion
#[derive(Debug, Clone)]
pub struct L2TOptions {
    /// Use shorthand symbols (e.g., `->` instead of `arrow.r`)
    /// Default: true
    pub prefer_shorthands: bool,

    /// Convert simple fractions to slash notation (e.g., `a/b` instead of `frac(a, b)`)
    /// Only applies to simple single-character numerator/denominator
    /// Default: true
    pub frac_to_slash: bool,

    /// Use `oo` instead of `infinity` for `\infty`
    /// Default: false
    pub infty_to_oo: bool,

    /// Preserve original spacing in the output
    /// Default: false
    pub keep_spaces: bool,

    /// Non-strict mode: allow unknown commands to pass through
    /// Default: true
    pub non_strict: bool,

    /// Apply output optimizations (e.g., `floor.l x floor.r` → `floor(x)`)
    /// Default: true
    pub optimize: bool,

    /// Expand LaTeX macros before parsing
    /// When true, macros defined with \newcommand, \def, etc. are expanded
    /// Default: true
    pub expand_macros: bool,
}

impl Default for L2TOptions {
    fn default() -> Self {
        Self {
            prefer_shorthands: true,
            frac_to_slash: true,
            infty_to_oo: false,
            keep_spaces: false,
            non_strict: true,
            optimize: true,
            expand_macros: true,
        }
    }
}

impl L2TOptions {
    /// Create new options with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create options optimized for human readability
    pub fn readable() -> Self {
        Self {
            prefer_shorthands: true,
            frac_to_slash: true,
            infty_to_oo: true,
            keep_spaces: false,
            non_strict: true,
            optimize: true,
            expand_macros: true,
        }
    }

    /// Create options for maximum compatibility (verbose output)
    pub fn verbose() -> Self {
        Self {
            prefer_shorthands: false,
            frac_to_slash: false,
            infty_to_oo: false,
            keep_spaces: false,
            non_strict: true,
            optimize: false,
            expand_macros: true,
        }
    }

    /// Create strict mode options (errors on unknown commands)
    pub fn strict() -> Self {
        Self {
            non_strict: false,
            ..Self::default()
        }
    }

    /// Create options with macro expansion disabled
    pub fn no_expand() -> Self {
        Self {
            expand_macros: false,
            ..Self::default()
        }
    }
}

lazy_static! {
    /// Merged command specification for parsing
    pub static ref MERGED_SPEC: CommandSpec = {
        let mut commands: FxHashMap<String, _> = DEFAULT_SPEC
            .items()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();

        for (k, v) in TEX_COMMAND_SPEC.items() {
            commands.insert(k.to_string(), v.clone());
        }

        CommandSpec::new(commands)
    };
}

/// Conversion mode (text vs math)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConversionMode {
    #[default]
    Text,
    Math,
}

/// Current environment context
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EnvironmentContext {
    #[default]
    None,
    Document,
    Bibliography,
    Figure,
    Table,
    Tabular,
    Itemize,
    Enumerate,
    Description,
    Equation,
    Align,
    Matrix,
    Cases,
    TikZ,
    Verbatim,
    Savequote,
    Theorem(String), // Theorem-like environment with name
}

/// Macro definition
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub num_args: usize,
    pub default_arg: Option<String>,
    pub replacement: String,
}

/// Pending operator state (for operatorname*)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOperator {
    pub is_limits: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingCaptureMode {
    None,
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CitationMode {
    Typst,
    Text,
}

impl Default for CitationMode {
    fn default() -> Self {
        CitationMode::Typst
    }
}

/// Pending section heading when arguments are parsed separately
#[derive(Debug, Clone)]
pub struct PendingHeading {
    pub level: u8,
    pub optional: Option<String>,
    pub required: Option<String>,
    pub capture_mode: HeadingCaptureMode,
    pub capture_depth: usize,
    pub capture_buffer: String,
    pub implicit_open: bool,
}

#[derive(Debug, Default, Clone)]
pub struct PageMargin {
    pub all: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct HeaderConfig {
    pub enabled: bool,
    pub left: Option<String>,
    pub center: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct HeadingStyleDef {
    pub size: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

impl PageMargin {
    pub fn to_typst(&self) -> Option<String> {
        if self.all.is_none()
            && self.left.is_none()
            && self.right.is_none()
            && self.top.is_none()
            && self.bottom.is_none()
        {
            return None;
        }
        if self.left.is_none()
            && self.right.is_none()
            && self.top.is_none()
            && self.bottom.is_none()
            && self.all.is_some()
        {
            return self.all.clone();
        }
        let left = self.left.as_ref().or(self.all.as_ref());
        let right = self.right.as_ref().or(self.all.as_ref());
        let top = self.top.as_ref().or(self.all.as_ref());
        let bottom = self.bottom.as_ref().or(self.all.as_ref());
        let mut parts = Vec::new();
        if let Some(v) = left {
            parts.push(format!("left: {}", v));
        }
        if let Some(v) = right {
            parts.push(format!("right: {}", v));
        }
        if let Some(v) = top {
            parts.push(format!("top: {}", v));
        }
        if let Some(v) = bottom {
            parts.push(format!("bottom: {}", v));
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!("({})", parts.join(", ")))
        }
    }
}

/// Conversion state maintained during AST traversal
#[derive(Debug, Default)]
pub struct ConversionState {
    /// Current conversion mode
    pub mode: ConversionMode,
    /// Stack of environment contexts
    pub env_stack: Vec<EnvironmentContext>,
    /// Indentation level (for lists)
    pub indent: usize,
    /// Collected labels for the current element
    pub pending_label: Option<String>,
    /// Known labels found in the document (sanitized)
    pub known_labels: HashSet<String>,
    /// Pending operator state
    pub pending_op: Option<PendingOperator>,
    /// Pending section heading awaiting arguments
    pub pending_heading: Option<PendingHeading>,
    /// User-defined macros
    pub macros: HashMap<String, MacroDef>,
    /// Cache for expanded macros with no arguments
    pub macro_cache: HashMap<String, String>,
    /// Whether we're in preamble
    pub in_preamble: bool,
    /// Document metadata
    pub title: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub document_class: Option<String>,
    pub document_class_info: Option<DocumentClass>,
    pub template_kind: Option<TemplateKind>,
    pub template_package: Option<String>,
    pub cvpr_review: bool,
    pub cvpr_final: bool,
    pub cvpr_paper_id: Option<String>,
    pub cvpr_conf_year: Option<String>,
    pub iclr_final: bool,
    pub icml_accepted: bool,
    pub neurips_preprint: bool,
    pub neurips_final: bool,
    pub tmlr_preprint: bool,
    pub tmlr_accepted: bool,
    pub rlj_preprint: bool,
    pub rlj_final: bool,
    pub aaai_anonymous: bool,
    pub abstract_text: Option<String>,
    pub keywords: Vec<String>,
    pub author_blocks: Vec<AuthorBlock>,
    pub current_author_idx: Option<usize>,
    pub affiliation_map: HashMap<String, String>,
    pub thesis_meta: Vec<(String, String)>,
    /// Collected structured warnings
    pub structured_warnings: Vec<ConversionWarning>,
    /// Legacy string warnings (for compatibility)
    pub warnings: Vec<String>,
    /// Collected conversion losses
    pub losses: Vec<LossRecord>,
    /// Incrementing loss id counter
    pub loss_seq: usize,
    /// Counter for theorems, equations, etc.
    pub counters: HashMap<String, u32>,
    /// Acronym definitions (key -> AcronymDef)
    pub acronyms: HashMap<String, AcronymDef>,
    /// Glossary definitions (key -> GlossaryDef)
    pub glossary: HashMap<String, GlossaryDef>,
    /// Set of acronyms that have been used (for first-use tracking)
    pub used_acronyms: HashSet<String>,
    /// Conversion options
    pub options: L2TOptions,
    /// Citation rendering mode
    pub citation_mode: CitationMode,
    /// Custom theorem environments defined in preamble
    pub custom_theorems: HashMap<String, String>,
    /// Color definitions collected from preamble (name -> Typst color expression)
    pub color_defs: Vec<(String, String)>,
    /// Page margin overrides collected from preamble
    pub page_margin: PageMargin,
    /// Optional paper override from geometry or similar
    pub page_paper: Option<String>,
    /// Paragraph spacing collected from preamble
    pub par_skip: Option<String>,
    pub par_indent: Option<String>,
    pub line_spacing: Option<String>,
    /// Link color from hyperref/hypersetup
    pub link_color: Option<String>,
    /// Page numbering style (Typst numbering pattern)
    pub page_numbering: Option<String>,
    /// Header configuration from fancyhdr-like commands
    pub header: HeaderConfig,
    /// Heading style overrides from titlesec
    pub heading_styles: HashMap<u8, HeadingStyleDef>,
    /// Profiling flags and counters (debug)
    pub profile_enabled: bool,
    pub profile_nodes: usize,
    pub profile_step: usize,
    pub profile_last: Option<String>,
    pub profile_last_env: Option<String>,
    pub profile_start: Option<Instant>,
    /// Current recursion depth for visit_node/visit_element (stack overflow protection)
    pub visit_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    Ieee,
    Acm,
    Lncs,
    Elsevier,
    Springer,
    Aaai,
    Cvpr,
    Iclr,
    Icml,
    Neurips,
    Jmlr,
    Tmlr,
    Rlj,
    MitThesis,
    StanfordThesis,
    UcbThesis,
    Dissertate,
}

#[derive(Debug, Clone, Default)]
pub struct AuthorBlock {
    pub name: Option<String>,
    pub lines: Vec<String>,
    pub email: Option<String>,
    pub affiliation_keys: Vec<String>,
}

impl ConversionState {
    /// Add a structured warning
    pub fn add_warning(&mut self, warning: ConversionWarning) {
        self.structured_warnings.push(warning);
    }

    /// Take all structured warnings
    pub fn take_structured_warnings(&mut self) -> Vec<ConversionWarning> {
        dedupe_structured_warnings(std::mem::take(&mut self.structured_warnings))
    }
}

impl ConversionState {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_loss_id(&mut self) -> String {
        self.loss_seq += 1;
        format!("L{:04}", self.loss_seq)
    }

    /// Push a new environment onto the stack
    pub fn push_env(&mut self, env: EnvironmentContext) {
        if matches!(
            env,
            EnvironmentContext::Itemize | EnvironmentContext::Enumerate
        ) {
            self.indent += 2;
        }
        self.env_stack.push(env);
    }

    /// Pop the current environment from the stack
    pub fn pop_env(&mut self) -> Option<EnvironmentContext> {
        let env = self.env_stack.pop();
        if let Some(ref e) = env {
            if matches!(
                e,
                EnvironmentContext::Itemize | EnvironmentContext::Enumerate
            ) {
                self.indent = self.indent.saturating_sub(2);
            }
        }
        env
    }

    /// Get current environment
    pub fn current_env(&self) -> &EnvironmentContext {
        self.env_stack.last().unwrap_or(&EnvironmentContext::None)
    }

    /// Check if we're in a specific environment type anywhere in the stack
    pub fn is_inside(&self, env: &EnvironmentContext) -> bool {
        self.env_stack
            .iter()
            .any(|e| std::mem::discriminant(e) == std::mem::discriminant(env))
    }

    /// Get next counter value
    pub fn next_counter(&mut self, name: &str) -> u32 {
        let counter = self.counters.entry(name.to_string()).or_insert(0);
        *counter += 1;
        *counter
    }

    /// Register an acronym definition
    pub fn register_acronym(&mut self, key: &str, short: &str, long: &str) {
        self.acronyms
            .insert(key.to_string(), AcronymDef::new(short, long));
    }

    /// Register a glossary entry
    pub fn register_glossary(&mut self, key: &str, name: &str, description: &str) {
        self.glossary
            .insert(key.to_string(), GlossaryDef::new(name, description));
    }

    /// Register or update a color definition for Typst output.
    pub fn register_color_def(&mut self, name: String, value: String) {
        if name.trim().is_empty() || value.trim().is_empty() {
            return;
        }
        if let Some(existing) = self.color_defs.iter_mut().find(|(n, _)| n == &name) {
            existing.1 = value;
        } else {
            self.color_defs.push((name, value));
        }
    }

    /// Get acronym and mark as used, returns (text, is_first_use)
    pub fn use_acronym(&mut self, key: &str) -> Option<(String, bool)> {
        if let Some(acr) = self.acronyms.get(key) {
            let is_first = !self.used_acronyms.contains(key);
            self.used_acronyms.insert(key.to_string());
            let text = if is_first {
                acr.full() // First use: "Long Form (SF)"
            } else {
                acr.short.clone() // Subsequent use: "SF"
            };
            Some((text, is_first))
        } else {
            None
        }
    }

    /// Get acronym short form only
    pub fn get_acronym_short(&self, key: &str) -> Option<String> {
        self.acronyms.get(key).map(|a| a.short.clone())
    }

    /// Get acronym long form only
    pub fn get_acronym_long(&self, key: &str) -> Option<String> {
        self.acronyms.get(key).map(|a| a.long.clone())
    }

    /// Get acronym full form
    pub fn get_acronym_full(&self, key: &str) -> Option<String> {
        self.acronyms.get(key).map(|a| a.full())
    }

    /// Get glossary entry name
    pub fn get_glossary_name(&self, key: &str) -> Option<String> {
        self.glossary.get(key).map(|g| g.name.clone())
    }
}

/// The main AST-based converter
pub struct LatexConverter {
    pub(crate) state: ConversionState,
    pub(crate) spec: CommandSpec,
}

impl LatexConverter {
    /// Create a new converter with default options
    pub fn new() -> Self {
        Self {
            state: ConversionState::new(),
            spec: MERGED_SPEC.clone(),
        }
    }

    /// Create a new converter with custom options
    pub fn with_options(options: L2TOptions) -> Self {
        let mut state = ConversionState::new();
        state.options = options;
        Self {
            state,
            spec: MERGED_SPEC.clone(),
        }
    }

    /// Get a reference to the current options
    pub fn options(&self) -> &L2TOptions {
        &self.state.options
    }

    /// Get a mutable reference to the current options
    pub fn options_mut(&mut self) -> &mut L2TOptions {
        &mut self.state.options
    }

    /// Preprocess input with optional macro expansion
    ///
    /// If `expand_macros` is enabled in options, this will:
    /// 1. Tokenize the input
    /// 2. Expand all macro definitions and invocations
    /// 3. Collect any warnings from the expansion process
    /// 4. Return the expanded string
    ///
    /// Otherwise, returns the input unchanged.
    fn preprocess_expansion(&mut self, input: &str, math_mode: bool) -> String {
        if self.state.options.expand_macros {
            let result =
                crate::core::latex2typst::engine::expand_latex_with_warnings(input, math_mode);

            // Convert structured engine warnings to conversion warnings (type-safe!)
            for engine_warning in result.warnings {
                if matches!(
                    &engine_warning,
                    EngineWarning::LetTargetNotFound { target, .. }
                        if is_benign_let_target(target)
                ) {
                    continue;
                }
                let suppress_string_warning = matches!(
                    &engine_warning,
                    EngineWarning::UnsupportedPrimitive { name }
                        if is_benign_unsupported_primitive(name)
                ) || matches!(
                    &engine_warning,
                    EngineWarning::DepthExceeded { .. } | EngineWarning::TokenLimitExceeded { .. }
                );
                let warning = Self::convert_engine_warning(&engine_warning);
                // Keep legacy string warning for compatibility
                if !suppress_string_warning {
                    self.state.warnings.push(engine_warning.message());
                }
                self.state.structured_warnings.push(warning);
            }

            result.output
        } else {
            input.to_string()
        }
    }

    /// Seed user-defined macros from raw input, so we can expand simple macros
    /// even if the full macro expansion engine bails out.
    fn seed_macros_from_input(&mut self, input: &str) {
        let tokens = tokenize(input);
        let (defs, _rest) = parse_definitions(tokens);
        for def in defs {
            match def {
                DefinitionKind::NewCommand {
                    name,
                    num_args,
                    default,
                    body,
                }
                | DefinitionKind::RenewCommand {
                    name,
                    num_args,
                    default,
                    body,
                }
                | DefinitionKind::ProvideCommand {
                    name,
                    num_args,
                    default,
                    body,
                } => {
                    let replacement = detokenize(&body);
                    let default_arg = default.map(|t| detokenize(&t));
                    self.state.macros.insert(
                        name.clone(),
                        MacroDef {
                            name,
                            num_args: num_args as usize,
                            default_arg,
                            replacement,
                        },
                    );
                }
                DefinitionKind::Def {
                    name,
                    signature,
                    body,
                }
                | DefinitionKind::Edef {
                    name,
                    signature,
                    body,
                } => {
                    let replacement = detokenize(&body);
                    let num_args = signature.num_args() as usize;
                    self.state.macros.insert(
                        name.clone(),
                        MacroDef {
                            name,
                            num_args,
                            default_arg: None,
                            replacement,
                        },
                    );
                }
                DefinitionKind::Let { name, target } => {
                    if let Some(existing) = self.state.macros.get(&target).cloned() {
                        let mut cloned = existing.clone();
                        cloned.name = name.clone();
                        self.state.macros.insert(name, cloned);
                    }
                }
                DefinitionKind::DeclareMathOperator {
                    name,
                    body,
                    is_starred: _,
                } => {
                    let op = detokenize(&body);
                    let replacement = format!("\\operatorname{{{}}}", op.trim());
                    self.state.macros.insert(
                        name.clone(),
                        MacroDef {
                            name,
                            num_args: 0,
                            default_arg: None,
                            replacement,
                        },
                    );
                }
                DefinitionKind::NewEnvironment { .. }
                | DefinitionKind::RenewEnvironment { .. }
                | DefinitionKind::NewIf { .. } => {
                    // ignore for now
                }
            }
        }
    }

    /// Convert a structured engine warning to a conversion warning.
    ///
    /// This is a type-safe mapping - no string parsing required!
    fn convert_engine_warning(warning: &EngineWarning) -> ConversionWarning {
        match warning {
            EngineWarning::DepthExceeded { max_depth } => ConversionWarning::new(
                WarningKind::MacroLoop,
                format!(
                    "Macro expansion depth exceeded maximum ({}). Possible infinite recursion.",
                    max_depth
                ),
            ),
            EngineWarning::TokenLimitExceeded { max_tokens } => ConversionWarning::new(
                WarningKind::MacroLoop,
                format!(
                    "Macro expansion produced too many tokens (exceeded {}). Possible infinite loop or exponential expansion.",
                    max_tokens
                ),
            ),
            EngineWarning::ArgumentParsingFailed {
            macro_name,
            error_kind,
        } => {
            let kind = match error_kind {
                ArgumentErrorType::RunawayArgument => WarningKind::RunawayArgument,
                ArgumentErrorType::PatternMismatch => WarningKind::PatternMismatch,
                ArgumentErrorType::Other(_) => WarningKind::ParseError,
            };
            ConversionWarning::new(
                kind,
                format!(
                    "Macro '\\{}' argument parsing failed: {}",
                    macro_name, error_kind
                ),
            )
            .with_location(format!("\\{}", macro_name))
        }
            EngineWarning::LaTeX3Skipped { token_count } => ConversionWarning::new(
                WarningKind::LaTeX3Skipped,
                format!(
                    "LaTeX3 block (\\ExplSyntaxOn ... \\ExplSyntaxOff) skipped ({} tokens). \
                        LaTeX3/expl3 syntax is not supported.",
                    token_count
                ),
            ),
            EngineWarning::UnsupportedPrimitive { name } => ConversionWarning::new(
                WarningKind::UnsupportedPrimitive,
                format!(
                    "Unsupported TeX primitive '\\{}' encountered. \
                        This may produce incorrect output.",
                    name
                ),
            )
            .with_location(format!("\\{}", name)),
            EngineWarning::LetTargetNotFound { name, target } => ConversionWarning::new(
                WarningKind::UnsupportedMacro,
                format!(
                    "\\let\\{}\\{}: target '\\{}' not found. \
                        Built-in LaTeX commands cannot be copied with \\let.",
                    name, target, target
                ),
            )
            .with_location(format!("\\let\\{}\\{}", name, target)),
        }
    }

    /// Check if input contains a real `\begin{document}` that is not commented out.
    ///
    /// This function scans line-by-line, ignoring lines where `\begin{document}`
    /// appears after a `%` comment marker.
    fn has_real_begin_document(input: &str) -> bool {
        for line in input.lines() {
            // Find position of \begin{document} in this line
            if let Some(doc_pos) = line.find("\\begin{document}") {
                // Check if there's a % comment before it
                let before_doc = &line[..doc_pos];
                // If % exists before \begin{document}, this line is commented
                if !before_doc.contains('%') {
                    return true;
                }
            }
        }
        false
    }

    /// Convert a complete LaTeX document to Typst
    pub fn convert_document(&mut self, input: &str) -> String {
        self.state.warnings.clear();
        self.state.structured_warnings.clear();
        self.state.losses.clear();
        self.state.loss_seq = 0;
        // Only enter preamble mode if there's actually a \begin{document}
        // that is NOT inside a comment. This avoids false positives from:
        //   % \begin{document}  (commented out)
        //   \begin{verbatim}\begin{document}\end{verbatim}  (inside verbatim - rare edge case)
        self.state.in_preamble = Self::has_real_begin_document(input);
        self.state.macro_cache.clear();
        self.state.macros.clear();
        let timing_enabled = std::env::var("TYLAX_TIMING").is_ok();
        self.state.profile_enabled = std::env::var("TYLAX_PROFILE").is_ok();
        self.state.profile_nodes = 0;
        self.state.profile_step = std::env::var("TYLAX_PROFILE_EVERY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(2000);
        self.state.profile_last = None;
        self.state.profile_last_env = None;
        self.state.profile_start = if self.state.profile_enabled {
            Some(Instant::now())
        } else {
            None
        };
        let mut last_mark = Instant::now();
        let mark_timing = |label: &str, last: &mut Instant, enabled: bool| {
            if enabled {
                let now = Instant::now();
                let secs = (now.duration_since(*last)).as_secs_f64();
                eprintln!("[tylax] {}: {:.3}s", label, secs);
                *last = now;
            }
        };
        if timing_enabled {
            eprintln!("[tylax] start");
        }

        // Preprocess: normalize \verb into a brace-based form so the parser can handle it.
        let verb_expanded = replace_verb_commands(input);
        // Preprocess: replace empty superscript math blocks like $^{th}$
        let verb_expanded = super::utils::replace_empty_math_superscripts(&verb_expanded);
        let verb_expanded = super::utils::replace_coloremojicode(&verb_expanded);
        // Preprocess: protect zero-argument commands that MiTeX would otherwise lose
        let protected_input = protect_zero_arg_commands(&verb_expanded);

        self.capture_preamble_hints(&protected_input);

        self.seed_macros_from_input(&protected_input);

        // Optionally expand macros using the token-based engine
        let mut expanded_input = self.preprocess_expansion(&protected_input, false);
        // Strip bibliography commands that are not meaningful in Typst output.
        expanded_input =
            super::utils::strip_command_with_braced_arg(&expanded_input, "bibliographystyle");
        expanded_input =
            super::utils::strip_command_with_braced_arg(&expanded_input, "thispagestyle");
        expanded_input = super::utils::strip_command_with_braced_arg(&expanded_input, "pagestyle");
        expanded_input = super::utils::strip_sectioning_stars(&expanded_input);
        expanded_input = super::utils::strip_env_stars(&expanded_input);
        expanded_input = super::utils::normalize_citation_optional_args(&expanded_input);
        expanded_input = super::utils::normalize_spacing_primitives(&expanded_input);
        expanded_input = self.strip_ifdefined_blocks(&expanded_input);
        expanded_input = super::utils::normalize_display_dollars(&expanded_input);
        expanded_input = super::utils::normalize_math_delimiters(&expanded_input);
        expanded_input = super::utils::normalize_unmatched_braces(&expanded_input);
        if timing_enabled {
            eprintln!("[tylax] expanded size: {} bytes", expanded_input.len());
        }
        mark_timing("macro expand+normalize", &mut last_mark, timing_enabled);

        let has_bib_files = !super::utils::collect_bibliography_entries(&expanded_input).is_empty();
        let has_thebibliography = super::utils::contains_thebibliography_env(&expanded_input);
        self.state.citation_mode = if has_bib_files {
            CitationMode::Typst
        } else if has_thebibliography {
            CitationMode::Text
        } else {
            CitationMode::Text
        };
        expanded_input = super::utils::strip_env_options(
            &expanded_input,
            &[
                "nomenclature",
                "nomenclature*",
                "algorithm",
                "algorithmic",
                "algorithm*",
                "algorithmic*",
                "itemize",
                "itemize*",
                "enumerate",
                "enumerate*",
                "description",
                "description*",
                "list",
                "list*",
                "compactitem",
                "compactenum",
                "compactdesc",
            ],
        );
        expanded_input = super::utils::strip_command_optional_arg(&expanded_input, &["blindtext"]);
        let mut doc_class = parse_document_class(&expanded_input);
        mark_timing("class parse", &mut last_mark, timing_enabled);
        let pkg_template = detect_template_from_packages(&expanded_input);
        let class_lower = doc_class.class_name.to_lowercase();
        let mut template_kind = match class_lower.as_str() {
            "ieeetran" => Some(TemplateKind::Ieee),
            "acmart" => Some(TemplateKind::Acm),
            "llncs" => Some(TemplateKind::Lncs),
            "elsarticle" => Some(TemplateKind::Elsevier),
            "svjour" | "svjour3" | "svproc" => Some(TemplateKind::Springer),
            "mitthesis" => Some(TemplateKind::MitThesis),
            "ucbthesis" => Some(TemplateKind::UcbThesis),
            "dissertate" => Some(TemplateKind::Dissertate),
            _ => None,
        };
        if template_kind.is_none() {
            template_kind = pkg_template.as_ref().map(|(kind, _)| *kind);
        }
        if matches!(
            template_kind,
            Some(
                TemplateKind::Cvpr
                    | TemplateKind::Iclr
                    | TemplateKind::Icml
                    | TemplateKind::Neurips
                    | TemplateKind::Aaai
            )
        ) && doc_class.columns <= 1
        {
            doc_class.columns = 2;
            if doc_class.paper.is_none() {
                doc_class.paper = Some("us-letter".to_string());
            }
        }
        self.state.document_class_info = Some(doc_class.clone());
        self.state.template_kind = template_kind;
        self.state.template_package = pkg_template.map(|(_, name)| name);
        // Debug: save expanded input if requested
        if let Ok(path) = std::env::var("TYLAX_DEBUG_EXPANDED") {
            let _ = std::fs::write(&path, &expanded_input);
            eprintln!("[tylax] debug: expanded input saved to {}", path);
        }
        // Parse with mitex-parser
        let tree = mitex_parser::parse(&expanded_input, self.spec.clone());
        mark_timing("parse", &mut last_mark, timing_enabled);

        // Convert AST to Typst with pre-allocated buffer
        let estimated_size = (expanded_input.len() as f64 * 1.5) as usize;
        let mut output = String::with_capacity(estimated_size.max(1024));

        // Walk the tree
        self.visit_node(&tree, &mut output);
        mark_timing("convert", &mut last_mark, timing_enabled);

        // Build final document with preamble
        let result = self.build_document(output);
        mark_timing("build document", &mut last_mark, timing_enabled);
        let resolved = resolve_reference_markers(&result);
        let attached = attach_orphan_labels(&resolved);
        let escaped = super::utils::escape_at_in_words(&attached);
        let normalized = super::utils::normalize_latex_quotes(&escaped);
        let sanitized = super::utils::sanitize_loss_comment_boundaries(&normalized);
        let sanitized = super::utils::normalize_typst_double_dollars(&sanitized);
        let sanitized = super::utils::normalize_typst_linebreaks(&sanitized);
        let sanitized = super::utils::normalize_typst_op_brackets(&sanitized);

        // Restore protected commands
        restore_protected_commands(&sanitized)
    }

    /// Convert math-only LaTeX to Typst
    pub fn convert_math(&mut self, input: &str) -> String {
        self.state.warnings.clear();
        self.state.losses.clear();
        self.state.loss_seq = 0;
        self.state.mode = ConversionMode::Math;
        self.state.in_preamble = false;

        // Optionally expand macros with math mode enabled
        let expanded_input = self.preprocess_expansion(input, true);

        // Parse
        let tree = mitex_parser::parse(&expanded_input, self.spec.clone());

        // Convert with pre-allocated buffer
        let mut output = String::with_capacity(expanded_input.len().max(256));
        self.visit_node(&tree, &mut output);

        // Post-process
        self.postprocess_math(output)
    }

    /// Convert inline math content (e.g., from embedded $x$ in \text{})
    /// This is a lightweight conversion that doesn't reset state
    pub fn convert_inline_math(&mut self, input: &str) -> String {
        let old_mode = self.state.mode.clone();
        self.state.mode = ConversionMode::Math;

        // Parse the math content
        let tree = mitex_parser::parse(input, self.spec.clone());

        // Convert
        let mut output = String::with_capacity(input.len().max(64));
        self.visit_node(&tree, &mut output);

        // Restore mode
        self.state.mode = old_mode;

        // Light post-processing - just trim
        output.trim().to_string()
    }

    /// Convert a complete LaTeX document and return a loss report
    pub fn convert_document_with_report(
        &mut self,
        input: &str,
    ) -> crate::utils::loss::ConversionReport {
        let content = self.convert_document(input);
        let report = self.take_loss_report();
        crate::utils::loss::ConversionReport::new(content, report)
    }

    /// Convert math-only LaTeX and return a loss report
    pub fn convert_math_with_report(
        &mut self,
        input: &str,
    ) -> crate::utils::loss::ConversionReport {
        let content = self.convert_math(input);
        let report = self.take_loss_report();
        crate::utils::loss::ConversionReport::new(content, report)
    }

    /// Record a conversion loss and return its id.
    pub fn record_loss(
        &mut self,
        kind: LossKind,
        name: Option<String>,
        message: impl Into<String>,
        snippet: Option<String>,
        context: Option<String>,
    ) -> String {
        let id = self.state.next_loss_id();
        let record = LossRecord::new(id.clone(), kind, name, message, snippet, context);
        self.state.losses.push(record);
        id
    }

    /// Consume the current loss report.
    pub fn take_loss_report(&mut self) -> LossReport {
        let losses = std::mem::take(&mut self.state.losses);
        let warnings = dedupe_string_warnings(std::mem::take(&mut self.state.warnings));
        LossReport::new("latex", "typst", losses, warnings)
    }

    /// Visit a syntax node and convert it
    pub fn visit_node(&mut self, node: &SyntaxNode, output: &mut String) {
        if self.state.profile_enabled {
            self.state.profile_nodes = self.state.profile_nodes.saturating_add(1);
            let step = self.state.profile_step.max(1);
            if self.state.profile_nodes % step == 0 {
                let env_suffix = self
                    .state
                    .profile_last_env
                    .as_ref()
                    .map(|env| format!(", env: {}", env))
                    .unwrap_or_default();
                let elapsed = self
                    .state
                    .profile_start
                    .as_ref()
                    .map(|start| format!(" {:.1}s", start.elapsed().as_secs_f64()))
                    .unwrap_or_default();
                if let Some(ref last) = self.state.profile_last {
                    eprintln!(
                        "[tylax] visit_node: {} nodes (last: {}{}){}",
                        self.state.profile_nodes, last, env_suffix, elapsed
                    );
                } else {
                    eprintln!(
                        "[tylax] visit_node: {} nodes{}{}",
                        self.state.profile_nodes, env_suffix, elapsed
                    );
                }
            }
        }
        for child in node.children_with_tokens() {
            self.visit_element(child, output);
        }
    }

    /// Visit a syntax element (node or token)
    pub fn visit_element(&mut self, elem: SyntaxElement, output: &mut String) {
        // Stack overflow protection: limit recursion depth
        // Note: This is actual call stack depth, not document nesting depth
        const MAX_VISIT_DEPTH: usize = 2000;
        self.state.visit_depth += 1;
        if self.state.visit_depth > MAX_VISIT_DEPTH {
            self.state.visit_depth -= 1;
            if !self.state.warnings.iter().any(|w| w.contains("recursion limit")) {
                self.state
                    .warnings
                    .push("Parser recursion limit reached; some content may be truncated".to_string());
            }
            return;
        }

        // Use a macro to ensure depth is decremented on all exit paths
        macro_rules! return_dec {
            () => {{
                self.state.visit_depth -= 1;
                return;
            }};
        }

        use SyntaxKind::*;
        let _profile_guard = if self.state.profile_enabled {
            let label = match elem.kind() {
                ItemCmd => {
                    if let SyntaxElement::Node(node) = &elem {
                        if let Some(cmd) = CmdItem::cast(node.clone()) {
                            let name = cmd
                                .name_tok()
                                .map(|t| t.text().to_string())
                                .unwrap_or_default();
                            format!("cmd {}", name)
                        } else {
                            "cmd".to_string()
                        }
                    } else {
                        "cmd".to_string()
                    }
                }
                ItemEnv => {
                    if let SyntaxElement::Node(node) = &elem {
                        if let Some(env) = EnvItem::cast(node.clone()) {
                            let name = env
                                .name_tok()
                                .map(|t| t.text().to_string())
                                .unwrap_or_default();
                            format!("env {}", name)
                        } else {
                            "env".to_string()
                        }
                    } else {
                        "env".to_string()
                    }
                }
                other => format!("{:?}", other),
            };
            self.state.profile_last = Some(label.clone());
            Some(ElementProfileGuard::new(
                label,
                self.state.profile_last_env.clone(),
            ))
        } else {
            None
        };

        if self.state.in_preamble {
            match elem.kind() {
                ScopeRoot => {
                    if let SyntaxElement::Node(n) = elem {
                        self.visit_node(&n, output);
                    }
                }
                ItemCmd => {
                    super::markup::convert_command(self, elem, output);
                }
                ItemEnv => {
                    super::environment::convert_environment(self, elem, output);
                }
                _ => {}
            }
            return_dec!();
        }

        if self.consume_pending_heading(&elem, output) {
            return_dec!();
        }

        match elem.kind() {
            // Handle errors gracefully
            TokenError => {
                let text = match &elem {
                    SyntaxElement::Node(n) => n.text().to_string(),
                    SyntaxElement::Token(t) => t.text().to_string(),
                };
                let trimmed = text.trim();
                if text.contains("\\)") {
                    output.push_str(&text.replace("\\)", ")"));
                    return_dec!();
                }
                if text.contains("\\]") {
                    output.push_str(&text.replace("\\]", "]"));
                    return_dec!();
                }
                if trimmed == "spacing"
                    || trimmed == "arraystretch"
                    || trimmed == "eqnarray"
                    || trimmed == "eqnarray*"
                {
                    return_dec!();
                }
                if trimmed == "}"
                    || trimmed == "document"
                    || trimmed == "\\begin"
                    || trimmed == "\\end"
                {
                    return_dec!();
                }
                self.state.warnings.push(format!("Parse error: {}", text));
                let context = match self.state.mode {
                    ConversionMode::Math => Some("math".to_string()),
                    ConversionMode::Text => Some("text".to_string()),
                };
                self.record_loss(
                    LossKind::ParseError,
                    None,
                    "Parse error",
                    Some(text.clone()),
                    context,
                );
                let _ = write!(output, "/* LaTeX Error: {} */", text.replace("*/", "* /"));
            }

            // Root - always recurse
            ScopeRoot => {
                if let SyntaxElement::Node(n) = elem {
                    self.visit_node(&n, output);
                }
            }

            // Containers
            ItemText => {
                if let SyntaxElement::Node(n) = elem {
                    if n.children().next().is_none() {
                        let text = n.text().to_string();
                        if text.contains('\n') {
                            self.visit_node(&n, output);
                        } else if matches!(self.state.mode, ConversionMode::Math) {
                            // In math mode, separate letters with spaces (abc -> a b c)
                            // but keep digits together as numbers (123 -> 123, 0.5 -> 0.5)
                            let mut prev_was_digit_or_dot = false;
                            for c in text.chars() {
                                let is_digit_or_dot = c.is_ascii_digit() || c == '.';
                                // Add space before letter, or before digit if previous wasn't digit/dot
                                if !output.is_empty()
                                    && !output.ends_with(' ')
                                    && !output.ends_with('(')
                                    && !output.ends_with('[')
                                    && !output.ends_with('{')
                                {
                                    if c.is_ascii_alphabetic() {
                                        output.push(' ');
                                    } else if is_digit_or_dot && !prev_was_digit_or_dot {
                                        // Starting a new number after a letter
                                        output.push(' ');
                                    }
                                }
                                output.push(c);
                                // Add space after letters (for next character)
                                if c.is_ascii_alphabetic() {
                                    output.push(' ');
                                }
                                prev_was_digit_or_dot = is_digit_or_dot;
                            }
                        } else {
                            super::utils::escape_typst_text_into(&text, output);
                        }
                    } else {
                        self.visit_node(&n, output);
                    }
                }
            }
            ItemParen | ClauseArgument => {
                if let SyntaxElement::Node(n) = elem {
                    self.visit_node(&n, output);
                }
            }

            // Math formula
            ItemFormula => {
                super::math::convert_formula(self, elem, output);
            }

            // Curly group
            ItemCurly => {
                if self.state.in_preamble {
                    return_dec!();
                }
                super::math::convert_curly(self, elem, output);
            }

            // Left/Right delimiters
            ItemLR | ClauseLR => {
                super::math::convert_lr(self, elem, output);
            }

            // Attachment (subscript/superscript)
            ItemAttachComponent => {
                super::math::convert_attachment(self, elem, output);
            }

            // Command
            ItemCmd => {
                super::markup::convert_command(self, elem, output);
            }

            // Environment
            ItemEnv => {
                super::environment::convert_environment(self, elem, output);
            }

            // Plain word
            TokenWord => {
                if let SyntaxElement::Token(t) = elem {
                    let text = t.text();
                    if matches!(self.state.mode, ConversionMode::Math) {
                        // In math mode, separate letters with spaces (abc -> a b c)
                        // but keep digits together as numbers (123 -> 123, 0.5 -> 0.5)
                        let mut prev_was_digit_or_dot = false;
                        for c in text.chars() {
                            let is_digit_or_dot = c.is_ascii_digit() || c == '.';
                            // Add space before letter, or before digit if previous wasn't digit/dot
                            if !output.is_empty()
                                && !output.ends_with(' ')
                                && !output.ends_with('(')
                                && !output.ends_with('[')
                                && !output.ends_with('{')
                            {
                                if c.is_ascii_alphabetic() {
                                    output.push(' ');
                                } else if is_digit_or_dot && !prev_was_digit_or_dot {
                                    // Starting a new number after a letter
                                    output.push(' ');
                                }
                            }
                            output.push(c);
                            // Add space after letters (for next character)
                            if c.is_ascii_alphabetic() {
                                output.push(' ');
                            }
                            prev_was_digit_or_dot = is_digit_or_dot;
                        }
                    } else {
                        super::utils::escape_typst_text_into(text, output);
                    }
                }
            }

            // Whitespace
            TokenWhiteSpace => {
                if let SyntaxElement::Token(t) = elem {
                    output.push_str(t.text());
                }
            }

            // Line break
            TokenLineBreak => {
                if let SyntaxElement::Token(t) = elem {
                    output.push_str(t.text());
                    for _ in 0..self.state.indent {
                        output.push(' ');
                    }
                } else {
                    output.push('\n');
                }
            }

            // Newline command \\
            ItemNewLine => match self.state.current_env() {
                EnvironmentContext::Matrix => output.push_str("zws ;"),
                EnvironmentContext::Cases => output.push(','),
                EnvironmentContext::Align | EnvironmentContext::Equation => {
                    output.push_str(" \\ ");
                }
                EnvironmentContext::Tabular => output.push_str("|||ROW|||"),
                _ => output.push_str("\\ "),
            },

            // Ampersand (column separator)
            TokenAmpersand => match self.state.current_env() {
                EnvironmentContext::Matrix => output.push_str("zws, "),
                EnvironmentContext::Cases => output.push_str("& "),
                EnvironmentContext::Align => output.push_str("& "),
                EnvironmentContext::Tabular | EnvironmentContext::Table => {
                    output.push_str("|||CELL|||")
                }
                _ => output.push('&'),
            },

            // Special characters
            TokenTilde => {
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push_str("space.nobreak ");
                } else {
                    output.push(' ');
                }
            }
            TokenHash => output.push_str("\\#"),
            TokenDollar => {
                if !matches!(self.state.mode, ConversionMode::Math) {
                    output.push_str("\\$");
                }
            }
            TokenUnderscore => {
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push('_');
                } else {
                    output.push_str("\\_");
                }
            }
            TokenCaret => {
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push('^');
                } else {
                    output.push_str("\\^");
                }
            }
            TokenApostrophe => output.push('\''),
            TokenComma => output.push(','),
            TokenSlash => output.push('/'),
            TokenAsterisk => {
                if let Some(ref mut op) = self.state.pending_op {
                    op.is_limits = true;
                    return_dec!();
                }
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push('*');
                } else {
                    output.push_str("\\*");
                }
            }
            TokenAtSign => {
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push('@');
                } else {
                    output.push_str("\\@");
                }
            }
            TokenSemicolon => output.push(';'),
            TokenDitto => output.push('"'),
            TokenLParen => output.push('('),
            TokenRParen => output.push(')'),
            TokenLBracket => {
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push('[');
                }
            }
            TokenRBracket => {
                if matches!(self.state.mode, ConversionMode::Math) {
                    output.push(']');
                }
            }

            // Ignore these
            TokenLBrace | TokenRBrace | TokenBeginMath | TokenEndMath | TokenComment
            | ItemBlockComment | ClauseCommandName | ItemBegin | ItemEnd | ItemBracket => {}

            // Command symbol
            TokenCommandSym => {
                super::markup::convert_command_sym(self, elem, output);
            }

            // Typst code passthrough
            ItemTypstCode => {
                if let SyntaxElement::Node(n) = elem {
                    output.push_str(&n.text().to_string());
                }
            }
        }
        // Decrement depth counter on exit
        self.state.visit_depth -= 1;
    }

    // ============================================================
    // Argument extraction helpers
    // ============================================================

    /// Get a required argument from a command (raw text, strips braces)
    pub fn get_required_arg(&self, cmd: &CmdItem, index: usize) -> Option<String> {
        let mut required_count = 0;
        let cmd_name = cmd
            .name_tok()
            .map(|t| t.text().trim_start_matches('\\').to_string())
            .unwrap_or_default();
        let allow_star_arg = matches!(cmd_name.as_str(), "overset" | "underset" | "stackrel");
        for child in cmd.syntax().children() {
            if child.kind() == SyntaxKind::ClauseArgument {
                let is_bracket = child
                    .children()
                    .any(|c| c.kind() == SyntaxKind::ItemBracket);
                if !is_bracket {
                    let preview = extract_arg_content(&child);
                    if preview.trim() == "*" && !allow_star_arg {
                        continue;
                    }
                    if required_count == index {
                        return Some(preview);
                    }
                    required_count += 1;
                }
            }
        }
        None
    }

    /// Get a required argument preserving inner braces
    pub fn get_required_arg_with_braces(&self, cmd: &CmdItem, index: usize) -> Option<String> {
        let mut required_count = 0;
        for child in cmd.syntax().children() {
            if child.kind() == SyntaxKind::ClauseArgument {
                let is_curly = child.children().any(|c| c.kind() == SyntaxKind::ItemCurly);
                if is_curly {
                    if required_count == index {
                        return Some(extract_arg_content_with_braces(&child));
                    }
                    required_count += 1;
                }
            }
        }
        None
    }

    /// Get an optional argument from a command
    pub fn get_optional_arg(&self, cmd: &CmdItem, index: usize) -> Option<String> {
        let mut optional_count = 0;
        for child in cmd.syntax().children() {
            if child.kind() == SyntaxKind::ClauseArgument {
                let is_bracket = child
                    .children()
                    .any(|c| c.kind() == SyntaxKind::ItemBracket);
                if is_bracket {
                    if optional_count == index {
                        return Some(extract_arg_content(&child));
                    }
                    optional_count += 1;
                }
            }
        }
        None
    }

    /// Convert a required argument - recursively processes the content
    pub fn convert_required_arg(&mut self, cmd: &CmdItem, index: usize) -> Option<String> {
        let mut required_count = 0;
        let cmd_name = cmd
            .name_tok()
            .map(|t| t.text().trim_start_matches('\\').to_string())
            .unwrap_or_default();
        let allow_star_arg = matches!(cmd_name.as_str(), "overset" | "underset" | "stackrel");
        let children: Vec<_> = cmd.syntax().children().collect();
        for (pos, child) in children.iter().enumerate() {
            if child.kind() == SyntaxKind::ClauseArgument {
                let is_bracket = child
                    .children()
                    .any(|c| c.kind() == SyntaxKind::ItemBracket);
                if !is_bracket {
                    let preview = extract_arg_content(&child);
                    if preview.trim() == "*" && !allow_star_arg {
                        let has_more_required = children[pos + 1..].iter().any(|next| {
                            if next.kind() != SyntaxKind::ClauseArgument {
                                return false;
                            }
                            !next.children().any(|c| c.kind() == SyntaxKind::ItemBracket)
                        });
                        if has_more_required {
                            continue;
                        }
                    }
                    if required_count == index {
                        let mut output = String::new();
                        for content in child.children_with_tokens() {
                            match content.kind() {
                                SyntaxKind::TokenLBrace | SyntaxKind::TokenRBrace => continue,
                                _ => self.visit_element(content, &mut output),
                            }
                        }
                        return Some(output.trim().to_string());
                    }
                    required_count += 1;
                }
            }
        }
        None
    }

    /// Get a required argument from a command and convert it to Typst
    pub fn get_converted_required_arg(&mut self, cmd: &CmdItem, index: usize) -> Option<String> {
        let raw_text = self.get_required_arg_with_braces(cmd, index)?;
        Some(convert_caption_text(&raw_text))
    }

    /// Get optional argument from an environment
    pub fn get_env_optional_arg(&self, node: &SyntaxNode) -> Option<String> {
        for child in node.children() {
            if child.kind() == SyntaxKind::ItemBegin {
                for begin_child in child.children() {
                    if begin_child.kind() == SyntaxKind::ClauseArgument {
                        let has_bracket = begin_child
                            .children()
                            .any(|c| c.kind() == SyntaxKind::ItemBracket);
                        if has_bracket {
                            return Some(extract_arg_content(&begin_child));
                        }
                    }
                }
            }
        }
        None
    }

    /// Get a required argument from an environment
    pub fn get_env_required_arg(&self, node: &SyntaxNode, index: usize) -> Option<String> {
        let mut required_count = 0;
        for child in node.children() {
            if child.kind() == SyntaxKind::ClauseArgument {
                let is_curly = child.children().any(|c| c.kind() == SyntaxKind::ItemCurly);
                if is_curly {
                    if required_count == index {
                        return Some(extract_arg_content(&child));
                    }
                    required_count += 1;
                }
            }
        }
        None
    }

    /// Extract and convert argument for metadata (title, author, date)
    pub fn extract_metadata_arg(&mut self, cmd: &CmdItem) -> Option<String> {
        self.get_required_arg_with_braces(cmd, 0).map(|raw| {
            let converted = convert_caption_text(&raw);
            let normalized = converted
                .replace("\\\\", " ")
                .replace("\\ ", " ");
            super::utils::clean_whitespace(&normalized).trim().to_string()
        })
    }

    /// Extract and convert argument for author fields (preserve \\ and \and, drop footnotes)
    pub fn extract_author_arg(&mut self, cmd: &CmdItem) -> Option<String> {
        self.get_required_arg_with_braces(cmd, 0)
            .map(|raw| super::utils::convert_author_text(&raw).trim().to_string())
    }

    fn capture_preamble_hints(&mut self, input: &str) {
        let preamble = input
            .split("\\begin{document}")
            .next()
            .unwrap_or(input);
        let hint_block = extract_tylax_hint_block(preamble);
        let hint_source = if hint_block.is_empty() {
            preamble.to_string()
        } else {
            format!("{preamble}\n{hint_block}")
        };
        capture_geometry_hints(&mut self.state, &hint_source);
        capture_length_hints(&mut self.state, &hint_source);
        capture_fancyhdr_hints(&mut self.state, &hint_source);
        capture_titleformat_hints(&mut self.state, &hint_source);
        capture_pagenumbering_hints(&mut self.state, &hint_source);
        capture_hypersetup_hints(&mut self.state, &hint_source);
        capture_color_defs(&mut self.state, &hint_source);
        if preamble.contains("\\doublespacing") {
            self.state.line_spacing = Some("1.4em".to_string());
        } else if preamble.contains("\\onehalfspacing") {
            self.state.line_spacing = Some("0.8em".to_string());
        } else if preamble.contains("\\singlespacing") {
            self.state.line_spacing = None;
        }
        if let Some(value) = extract_macro_arg(preamble, "setstretch") {
            if let Ok(scale) = value.trim().parse::<f32>() {
                if scale > 1.0 {
                    let leading = scale - 1.0;
                    self.state.line_spacing = Some(format!("{:.3}em", leading));
                } else {
                    self.state.line_spacing = None;
                }
            }
        }
        let preamble_clean = strip_latex_comments(preamble);
        let preamble_lower = preamble_clean.to_ascii_lowercase();
        if preamble_lower.contains("\\def\\aaaianonymous")
            || preamble_lower.contains("\\aaaianonymoustrue")
        {
            self.state.aaai_anonymous = true;
        }
        if preamble_lower.contains("\\usepackage[review]{cvpr}") {
            self.state.cvpr_review = true;
            self.state.cvpr_final = false;
        }
        if preamble_lower.contains("\\usepackage{cvpr}")
            || preamble_lower.contains("\\usepackage[pagenumbers]{cvpr}")
        {
            self.state.cvpr_final = true;
        }
        if preamble_lower.contains("\\cvprfinalcopy") {
            self.state.cvpr_final = true;
            self.state.cvpr_review = false;
        }
        if preamble_lower.contains("\\iclrfinalcopy") {
            self.state.iclr_final = true;
        }
        if preamble_lower.contains("\\usepackage[accepted]{icml") {
            self.state.icml_accepted = true;
        }
        if preamble_lower.contains("\\usepackage[preprint]{neurips") {
            self.state.neurips_preprint = true;
        }
        if preamble_lower.contains("\\usepackage[final]{neurips") {
            self.state.neurips_final = true;
        }
        if preamble_lower.contains("\\usepackage[preprint]{tmlr}") {
            self.state.tmlr_preprint = true;
        }
        if preamble_lower.contains("\\usepackage[accepted]{tmlr}") {
            self.state.tmlr_accepted = true;
        }
        if preamble_lower.contains("\\usepackage[preprint]{rlj}")
            || preamble_lower.contains("\\usepackage[preprint]{rlc}")
        {
            self.state.rlj_preprint = true;
        }
        if preamble_lower.contains("\\usepackage[final]{rlj}")
            || preamble_lower.contains("\\usepackage[final]{rlc}")
            || preamble_lower.contains("\\usepackage[accepted]{rlj}")
            || preamble_lower.contains("\\usepackage[accepted]{rlc}")
        {
            self.state.rlj_final = true;
        }
        if let Some(id) = extract_def_macro(preamble, "paperID")
            .or_else(|| extract_def_macro(preamble, "cvprPaperID"))
        {
            self.state.cvpr_paper_id = Some(id);
        }
        if let Some(year) = extract_def_macro(preamble, "confYear") {
            self.state.cvpr_conf_year = Some(year);
        }
    }

    /// Extract inner content of a curly/bracket node, skipping its braces
    pub fn extract_curly_inner_content(&self, node: &SyntaxNode) -> String {
        extract_curly_inner_content(node)
    }

    // ============================================================
    // Math post-processing
    // ============================================================

    fn collapse_spaces(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut prev_space = false;
        for ch in input.chars() {
            if ch == ' ' {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
            } else {
                prev_space = false;
                out.push(ch);
            }
        }
        out
    }

    /// Post-process math output
    pub fn postprocess_math(&self, input: String) -> String {
        let mut result = input;

        result = self.fix_operatorname(&result);
        result = self.fix_blackboard_bold(&result);
        result = self.fix_empty_accent_args(&result);
        result = self.fix_symbol_spacing(&result);
        result = self.fix_multiletter_before_attachment(&result);
        result = self.collapse_spaces(&result);

        result = result.replace(" ,", ",");
        result = result.replace("( ", "(");
        result = result.replace(" )", ")");
        result = result.replace(" ^", "^");
        result = result.replace(" _", "_");

        result.trim().to_string()
    }

    /// Clean up math spacing
    pub fn cleanup_math_spacing(&self, input: &str) -> String {
        let mut result = self.collapse_spaces(input);

        result = result.replace(" ,", ",");
        result = result.replace("( ", "(");
        result = result.replace(" )", ")");
        result = result.replace(" (", "(");
        result = result.replace(" [", "[");
        result = result.replace(" ^", "^");
        result = result.replace(" _", "_");

        result
    }

    pub(crate) fn fix_multiletter_before_attachment(&self, input: &str) -> String {
        // Known Typst symbol names that should NOT be split
        static TYPST_SYMBOLS: &[&str] = &[
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
            "iota", "kappa", "lambda", "mu", "nu", "xi", "omicron", "pi", "rho",
            "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
            "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta",
            "Iota", "Kappa", "Lambda", "Mu", "Nu", "Xi", "Omicron", "Pi", "Rho",
            "Sigma", "Tau", "Upsilon", "Phi", "Chi", "Psi", "Omega",
            "infty", "infinity", "partial", "nabla", "forall", "exists",
            "emptyset", "nothing", "dots", "cdots", "ldots", "vdots", "ddots",
            "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan",
            "sinh", "cosh", "tanh", "coth", "log", "ln", "exp", "lim", "max", "min",
            "sup", "inf", "det", "dim", "ker", "deg", "gcd", "lcm", "mod", "arg",
            "sum", "prod", "product", "int", "integral", "oint", "iint", "iiint", "coprod",
        ];

        let chars: Vec<char> = input.chars().collect();
        let mut out = String::with_capacity(input.len());
        let mut i = 0usize;
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_ascii_alphabetic() {
                let start = i;
                let mut j = i;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let word: String = chars[start..j].iter().collect();
                let mut k = j;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                // Don't split if this is a known Typst symbol name
                let is_symbol = TYPST_SYMBOLS.contains(&word.as_str());
                if word.len() > 1 && k < chars.len() && (chars[k] == '_' || chars[k] == '^') && !is_symbol {
                    for (idx, letter) in word.chars().enumerate() {
                        if idx > 0 {
                            out.push(' ');
                        }
                        out.push(letter);
                    }
                } else {
                    out.push_str(&word);
                }
                i = j;
                continue;
            }
            out.push(ch);
            i += 1;
        }
        out
    }

    /// Fix missing spaces before Typst symbol names.
    ///
    /// When a non-letter character (digit, `/`, `)`, `]`, etc.) is immediately followed
    /// by a Typst symbol name (e.g., `angle.l`, `pi`, `theta`), insert a space.
    pub fn fix_symbol_spacing(&self, input: &str) -> String {
        // Common Typst symbol prefixes that need space separation
        // These are symbols that often appear after expressions without spaces
        static SYMBOL_PREFIXES: &[&str] = &[
            "chevron.l",
            "chevron.r",
            "floor.l",
            "floor.r",
            "ceil.l",
            "ceil.r",
            "bracket.l",
            "bracket.r",
            "paren.l",
            "paren.r",
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "zeta",
            "eta",
            "theta",
            "iota",
            "kappa",
            "lambda",
            "mu",
            "nu",
            "xi",
            "omicron",
            "pi",
            "rho",
            "sigma",
            "tau",
            "upsilon",
            "phi",
            "chi",
            "psi",
            "omega",
            "Alpha",
            "Beta",
            "Gamma",
            "Delta",
            "Epsilon",
            "Zeta",
            "Eta",
            "Theta",
            "Iota",
            "Kappa",
            "Lambda",
            "Mu",
            "Nu",
            "Xi",
            "Omicron",
            "Pi",
            "Rho",
            "Sigma",
            "Tau",
            "Upsilon",
            "Phi",
            "Chi",
            "Psi",
            "Omega",
            "infty",
            "infinity",
            "partial",
            "nabla",
            "forall",
            "exists",
            "emptyset",
            "nothing",
            "dots",
            "cdots",
            "ldots",
            "vdots",
            "ddots",
        ];

        let mut result = input.to_string();

        for symbol in SYMBOL_PREFIXES {
            // Pattern: non-letter/non-space followed by symbol
            // We need to find cases like "2angle.r" or ")pi"
            let mut i = 0;
            while i < result.len() {
                if let Some(pos) = result[i..].find(symbol) {
                    let abs_pos = i + pos;
                    if abs_pos > 0 {
                        let prev_char = result.chars().nth(abs_pos - 1).unwrap_or(' ');
                        // Insert space if previous char is not a letter, space, or opening paren/bracket
                        if !prev_char.is_alphabetic()
                            && prev_char != ' '
                            && prev_char != '('
                            && prev_char != '['
                            && prev_char != '{'
                            && prev_char != '\n'
                            && prev_char != '\t'
                        {
                            // Check that we're not in the middle of a word
                            // e.g., don't change "tangent" when looking for "angle"
                            let after_symbol = abs_pos + symbol.len();
                            let next_char = result.chars().nth(after_symbol);
                            let is_word_boundary =
                                next_char.is_none_or(|c| !c.is_alphanumeric() && c != '.');

                            if is_word_boundary {
                                result.insert(abs_pos, ' ');
                                i = abs_pos + symbol.len() + 2; // Skip past inserted space and symbol
                                continue;
                            }
                        }
                    }
                    i = abs_pos + 1;
                } else {
                    break;
                }
            }
        }

        result
    }

    /// Fix operatorname() patterns
    pub fn fix_operatorname(&self, input: &str) -> String {
        let mut result = input.to_string();
        let mut search = 0usize;

        while let Some(rel_start) = result[search..].find("operatorname(") {
            let start = search + rel_start;
            let after = &result[start + 13..];
            if let Some(end) = self.find_matching_paren(after) {
                let content = &after[..end];
                let clean_content: String =
                    content.chars().filter(|c| !c.is_whitespace()).collect();
                let replacement = format!("op(\"{}\")", clean_content);
                let total_end = start + 13 + end + 1;
                let existing = &result[start..total_end];
                if existing == replacement {
                    search = total_end;
                    continue;
                }
                result = format!(
                    "{}{}{}",
                    &result[..start],
                    replacement,
                    &result[total_end..]
                );
                search = start + replacement.len();
            } else {
                break;
            }
        }

        result
    }

    /// Fix bb() (blackboard bold)
    pub fn fix_blackboard_bold(&self, input: &str) -> String {
        let mut result = input.to_string();
        let mut search = 0usize;

        while let Some(rel_start) = result[search..].find("bb(") {
            let start = search + rel_start;
            let after = &result[start + 3..];
            if let Some(end) = self.find_matching_paren(after) {
                let content = &after[..end];
                let clean_content: String =
                    content.chars().filter(|c| !c.is_whitespace()).collect();

                let replacement = match clean_content.as_str() {
                    "E" => "EE".to_string(),
                    "P" => "PP".to_string(),
                    "R" => "RR".to_string(),
                    "N" => "NN".to_string(),
                    "Z" => "ZZ".to_string(),
                    "Q" => "QQ".to_string(),
                    "C" => "CC".to_string(),
                    _ => format!("bb({})", clean_content),
                };

                let total_end = start + 3 + end + 1;
                let existing = &result[start..total_end];
                if existing == replacement {
                    search = total_end;
                    continue;
                }
                result = format!(
                    "{}{}{}",
                    &result[..start],
                    replacement,
                    &result[total_end..]
                );
                search = start + replacement.len();
            } else {
                break;
            }
        }

        result
    }

    /// Fix empty accent/function patterns
    pub fn fix_empty_accent_args(&self, input: &str) -> String {
        let mut result = input.to_string();

        let accents = [
            "hat",
            "tilde",
            "bar",
            "vec",
            "dot",
            "ddot",
            "acute",
            "grave",
            "breve",
            "check",
            "overline",
            "underline",
            "widehat",
            "widetilde",
            "sqrt",
            "cancel",
            "bold",
            "italic",
            "cal",
            "frak",
            "bb",
            "mono",
            "sans",
        ];

        for accent in accents {
            let pattern = format!("{}()", accent);
            while let Some(pos) = result.find(&pattern) {
                let after = &result[pos + pattern.len()..];
                if let Some(first_char) = after.chars().next() {
                    if first_char.is_alphanumeric() {
                        let arg_end = self.find_simple_arg_end(after);
                        let arg = &after[..arg_end];
                        let replacement = format!("{}({})", accent, arg.trim());
                        let total = pos + pattern.len() + arg_end;
                        result = format!("{}{}{}", &result[..pos], replacement, &result[total..]);
                        continue;
                    }
                }
                break;
            }
        }

        result
    }

    /// Find matching closing parenthesis
    pub fn find_matching_paren(&self, s: &str) -> Option<usize> {
        let mut depth = 1;
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Find the end of a simple argument
    pub fn find_simple_arg_end(&self, s: &str) -> usize {
        let mut pos = 0;
        for c in s.chars() {
            if c.is_alphanumeric() || c == '_' {
                pos += c.len_utf8();
            } else {
                break;
            }
        }
        if pos == 0 {
            1
        } else {
            pos
        }
    }

    /// Check if a term is simple enough for slash notation
    pub fn is_simple_term(&self, s: &str) -> bool {
        let s = s.trim();
        if s.is_empty() {
            return false;
        }

        if s.len() == 1 {
            let c = s.chars().next().unwrap();
            return c.is_alphanumeric();
        }

        if s.len() <= 3 && s.chars().all(|c| c.is_alphanumeric()) {
            return true;
        }

        let simple_symbols = [
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
            "lambda", "mu", "nu", "xi", "pi", "rho", "sigma", "tau", "upsilon", "phi", "chi",
            "psi", "omega", "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta",
            "Iota", "Kappa", "Lambda", "Mu", "Nu", "Xi", "Pi", "Rho", "Sigma", "Tau", "Upsilon",
            "Phi", "Chi", "Psi", "Omega",
        ];

        if simple_symbols.contains(&s) {
            return true;
        }

        if s.contains('_') || s.contains('^') {
            let parts: Vec<&str> = s.split(['_', '^']).collect();
            if parts.len() == 2
                && parts[0].len() <= 2
                && parts[0].chars().all(|c| c.is_alphanumeric())
                && parts[1].len() <= 2
                && parts[1]
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '(' || c == ')')
            {
                return true;
            }
        }

        false
    }

    // ============================================================
    // Document building
    // ============================================================

    /// Build the final Typst document
    pub fn build_document(&self, content: String) -> String {
        let mut doc = String::new();

        let author_for_title = self.compose_author_string();

        // Document metadata
        if self.state.title.is_some() || author_for_title.is_some() {
            doc.push_str("#set document(\n");
            if let Some(ref title) = self.state.title {
                let _ = writeln!(doc, "  title: \"{}\",", title.replace('"', "\\\""));
            }
            if let Some(ref author) = author_for_title {
                let _ = writeln!(doc, "  author: \"{}\",", author.replace('"', "\\\""));
            }
            doc.push_str(")\n\n");
        }

        let doc_class = self.state.document_class_info.clone().unwrap_or_default();
        if !matches!(
            self.state.template_kind,
            Some(
                TemplateKind::Cvpr
                    | TemplateKind::Iclr
                    | TemplateKind::Icml
                    | TemplateKind::Neurips
                    | TemplateKind::Jmlr
                    | TemplateKind::Tmlr
                    | TemplateKind::Rlj
            )
        ) {
            doc.push_str(&generate_typst_preamble(&doc_class));
        }
        if !self.state.color_defs.is_empty() {
            for (name, value) in &self.state.color_defs {
                let _ = writeln!(doc, "#let {} = {}", name, value);
            }
            doc.push('\n');
        }
        if let Some(color) = self.state.link_color.as_deref() {
            let _ = writeln!(doc, "#show link: set text(fill: {})", color);
        }

        if let Some(paper) = self.state.page_paper.as_deref() {
            let typst_paper = normalize_paper_size(paper);
            let _ = writeln!(doc, "#set page(paper: \"{}\")", typst_paper);
        }
        if let Some(margin) = self.state.page_margin.to_typst() {
            let _ = writeln!(doc, "#set page(margin: {})", margin);
        }
        let mut par_args = Vec::new();
        if let Some(spacing) = self.state.par_skip.as_deref() {
            par_args.push(format!("spacing: {}", spacing));
        }
        if let Some(indent) = self.state.par_indent.as_deref() {
            par_args.push(format!("first-line-indent: {}", indent));
        }
        if let Some(leading) = self.state.line_spacing.as_deref() {
            par_args.push(format!("leading: {}", leading));
        }
        if !par_args.is_empty() {
            let _ = writeln!(doc, "#set par({})", par_args.join(", "));
        }
        if self.state.header.enabled {
            let left = self.state.header.left.as_deref().unwrap_or("").trim();
            let center = self.state.header.center.as_deref().unwrap_or("").trim();
            let right = self.state.header.right.as_deref().unwrap_or("").trim();
            if !left.is_empty() || !center.is_empty() || !right.is_empty() {
                doc.push_str("#set page(header: context {\n");
                doc.push_str("  if here().page() == 1 { return }\n");
                doc.push_str("  stack(spacing: 2pt,\n");
                if !center.is_empty() {
                    doc.push_str("    grid(columns: (1fr, 1fr, 1fr), align: (left, center, right),\n");
                    let _ = writeln!(
                        doc,
                        "      text(\"{}\"), text(\"{}\"), text(\"{}\"),\n    ),",
                        escape_typst_string(left),
                        escape_typst_string(center),
                        escape_typst_string(right)
                    );
                } else {
                    doc.push_str("    grid(columns: (1fr, 1fr), align: (left, right),\n");
                    let _ = writeln!(
                        doc,
                        "      text(\"{}\"), text(\"{}\"),\n    ),",
                        escape_typst_string(left),
                        escape_typst_string(right)
                    );
                }
                doc.push_str("    line(length: 100%, stroke: (thickness: 0.5pt)),\n");
                doc.push_str("  )\n");
                doc.push_str("})\n");
            }
        }
        if !doc_class.is_presentation() {
            let numbering = self
                .state
                .page_numbering
                .clone()
                .or_else(|| if self.state.header.enabled { Some("1".to_string()) } else { None });
            if let Some(numbering) = numbering {
                let _ = writeln!(doc, "#set page(numbering: \"{}\")", numbering);
                doc.push_str(
                    "#set page(footer: context { align(center, counter(page).display()) })\n",
                );
            }
        }
        if !self.state.heading_styles.is_empty() {
            let mut levels: Vec<_> = self.state.heading_styles.keys().copied().collect();
            levels.sort_unstable();
            for level in levels {
                if let Some(style) = self.state.heading_styles.get(&level) {
                    let mut args = Vec::new();
                    if let Some(size) = style.size.as_deref() {
                        args.push(format!("size: {}", size));
                    }
                    if style.bold {
                        args.push("weight: \"bold\"".to_string());
                    }
                    if style.italic {
                        args.push("style: \"italic\"".to_string());
                    }
                    if !args.is_empty() {
                        let _ = writeln!(
                            doc,
                            "#show heading.where(level: {}): set text({})",
                            level,
                            args.join(", ")
                        );
                    }
                }
            }
        }
        if doc.ends_with('\n') {
            doc.push('\n');
        }

        match self.state.template_kind {
            Some(TemplateKind::Ieee) => {
                doc.push_str(&self.render_ieee_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Acm) => {
                doc.push_str(&self.render_acm_show_rule(
                    self.state.title.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Lncs) => {
                doc.push_str(&self.render_lncs_show_rule(
                    self.state.title.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Elsevier) => {
                doc.push_str(&self.render_elsevier_show_rule(
                    self.state.title.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Springer) => {
                doc.push_str(&self.render_springer_show_rule(
                    self.state.title.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Cvpr) => {
                doc.push_str(&self.render_cvpr_block_open(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                ));
            }
            Some(TemplateKind::Iclr) => {
                doc.push_str(&self.render_iclr_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Icml) => {
                doc.push_str(&self.render_icml_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Neurips) => {
                doc.push_str(&self.render_neurips_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Jmlr) => {
                doc.push_str(&self.render_jmlr_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Tmlr) => {
                doc.push_str(&self.render_tmlr_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            Some(TemplateKind::Rlj) => {
                doc.push_str(&self.render_rlj_show_rule(
                    self.state.title.as_deref(),
                    self.state.author.as_deref(),
                    self.state.abstract_text.as_deref(),
                    &self.state.keywords,
                ));
            }
            _ => {
                let title_block = generate_title_block(
                    self.state.title.as_deref(),
                    author_for_title.as_deref(),
                    self.state.date.as_deref(),
                    self.state.abstract_text.as_deref(),
                );
                doc.push_str(&title_block);
            }
        }

        if matches!(
            self.state.template_kind,
            Some(
                TemplateKind::MitThesis
                    | TemplateKind::StanfordThesis
                    | TemplateKind::UcbThesis
                    | TemplateKind::Dissertate
            )
        ) {
            doc.push_str(&self.render_thesis_meta_block());
        }

        // Clean up content
        let cleaned_content = clean_whitespace(&content);
        doc.push_str(&cleaned_content);
        if self.state.template_kind == Some(TemplateKind::Cvpr) {
            doc.push_str("\n]\n");
        }

        // Add warnings as comments
        let warnings = dedupe_string_warnings(self.state.warnings.clone());
        if !warnings.is_empty() {
            doc.push_str("\n\n// Conversion warnings:\n");
            for warning in &warnings {
                let _ = writeln!(doc, "// - {}", warning);
            }
        }

        clean_whitespace(&doc)
    }

    fn compose_author_string(&self) -> Option<String> {
        if !self.state.author_blocks.is_empty() {
            let mut blocks = Vec::new();
            for block in &self.state.author_blocks {
                let mut lines = Vec::new();
                if let Some(name) = block.name.as_deref() {
                    if !name.trim().is_empty() {
                        lines.push(name.trim().to_string());
                    }
                }
                for line in &block.lines {
                    if !line.trim().is_empty() {
                        lines.push(line.trim().to_string());
                    }
                }
                if !block.affiliation_keys.is_empty() {
                    for key in &block.affiliation_keys {
                        if let Some(text) = self.state.affiliation_map.get(key) {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                lines.push(trimmed.to_string());
                            }
                        }
                    }
                }
                if let Some(email) = block.email.as_deref() {
                    if !email.trim().is_empty() {
                        lines.push(email.trim().to_string());
                    }
                }
                if !lines.is_empty() {
                    blocks.push(lines.join("\\\\"));
                }
            }
            if !blocks.is_empty() {
                return Some(blocks.join(" \\and "));
            }
        }
        self.state.author.clone()
    }

    pub fn push_author_block(&mut self, name: String) {
        let mut block = AuthorBlock::default();
        if !name.trim().is_empty() {
            block.name = Some(name.trim().to_string());
        }
        self.state.author_blocks.push(block);
        self.state.current_author_idx = Some(self.state.author_blocks.len().saturating_sub(1));
    }

    pub fn push_author_block_with_affils(&mut self, name: String, keys: Vec<String>) {
        let mut block = AuthorBlock::default();
        if !name.trim().is_empty() {
            block.name = Some(name.trim().to_string());
        }
        block.affiliation_keys = keys;
        self.state.author_blocks.push(block);
        self.state.current_author_idx = Some(self.state.author_blocks.len().saturating_sub(1));
    }

    pub fn add_author_line(&mut self, line: String) {
        let idx = match self.state.current_author_idx {
            Some(i) => i,
            None => return,
        };
        if let Some(block) = self.state.author_blocks.get_mut(idx) {
            if !line.trim().is_empty() {
                block.lines.push(line.trim().to_string());
            }
        }
    }

    pub fn add_author_email(&mut self, email: String) {
        let idx = match self.state.current_author_idx {
            Some(i) => i,
            None => return,
        };
        if let Some(block) = self.state.author_blocks.get_mut(idx) {
            if !email.trim().is_empty() {
                block.email = Some(email.trim().to_string());
            }
        }
    }

    pub fn capture_ieee_author_blocks(&mut self, raw: &str) -> bool {
        fn read_braced_content(input: &str) -> Option<(String, usize)> {
            let bytes = input.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'{' {
                return None;
            }
            let mut depth = 0usize;
            let mut start = None;
            for (idx, ch) in input[i..].char_indices() {
                match ch {
                    '{' => {
                        depth += 1;
                        if depth == 1 {
                            start = Some(i + idx + 1);
                        }
                    }
                    '}' => {
                        if depth > 0 {
                            depth -= 1;
                            if depth == 0 {
                                if let Some(s) = start {
                                    let end = i + idx;
                                    return Some((input[s..end].to_string(), end + 1));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let mut blocks: Vec<AuthorBlock> = Vec::new();
        let mut current: Option<AuthorBlock> = None;
        let mut saw_ieee = false;
        let mut idx = 0usize;

        while idx < raw.len() {
            let slice = &raw[idx..];
            if slice.starts_with("\\IEEEauthorblockN") {
                saw_ieee = true;
                if let Some(block) = current.take() {
                    if block.name.is_some() || !block.lines.is_empty() || block.email.is_some() {
                        blocks.push(block);
                    }
                }
                idx += "\\IEEEauthorblockN".len();
                if let Some((content, used)) = read_braced_content(&raw[idx..]) {
                    let name = super::utils::convert_author_text(&content);
                    let mut block = AuthorBlock::default();
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        block.name = Some(trimmed.to_string());
                    }
                    current = Some(block);
                    idx += used;
                    continue;
                }
            } else if slice.starts_with("\\IEEEauthorblockA") {
                saw_ieee = true;
                idx += "\\IEEEauthorblockA".len();
                if let Some((content, used)) = read_braced_content(&raw[idx..]) {
                    let mut block = current.take().unwrap_or_default();
                    let converted = super::utils::convert_author_text(&content);
                    let normalized = converted.replace("\\\\", "\n");
                    for line in normalized.lines() {
                        let cleaned = Self::normalize_author_field_line(line);
                        if cleaned.is_empty() {
                            continue;
                        }
                        if block.email.is_none() {
                            if let Some(found) = Self::extract_email_from_line(&cleaned) {
                                block.email = Some(found);
                                continue;
                            }
                        }
                        block.lines.push(cleaned);
                    }
                    current = Some(block);
                    idx += used;
                    continue;
                }
            } else if slice.starts_with("\\and") {
                saw_ieee = true;
                if let Some(block) = current.take() {
                    if block.name.is_some() || !block.lines.is_empty() || block.email.is_some() {
                        blocks.push(block);
                    }
                }
                idx += "\\and".len();
                continue;
            }
            idx += 1;
        }

        if let Some(block) = current.take() {
            if block.name.is_some() || !block.lines.is_empty() || block.email.is_some() {
                blocks.push(block);
            }
        }

        if !saw_ieee || blocks.is_empty() {
            return false;
        }
        self.state.author_blocks = blocks;
        self.state.current_author_idx = None;
        true
    }

    pub fn push_thesis_meta(&mut self, label: &str, value: String) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            self.state
                .thesis_meta
                .push((label.to_string(), trimmed.to_string()));
        }
    }

    pub fn add_affiliation_mapping(&mut self, key: String, value: String) {
        if !key.trim().is_empty() && !value.trim().is_empty() {
            self.state
                .affiliation_map
                .insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    pub fn set_author_email_by_name(&mut self, name: &str, email: String) -> bool {
        let target = name.trim();
        if target.is_empty() || email.trim().is_empty() {
            return false;
        }
        for block in &mut self.state.author_blocks {
            if let Some(block_name) = block.name.as_deref() {
                if block_name.trim() == target {
                    block.email = Some(email.trim().to_string());
                    return true;
                }
            }
        }
        false
    }

    pub fn capture_acm_affiliation(&mut self, raw: &str) {
        let mut lines = Vec::new();
        for key in ["institution", "department", "city", "country"] {
            if let Some(value) = extract_macro_arg(raw, key) {
                lines.push(value);
            }
        }
        if lines.is_empty() {
            let text = convert_caption_text(raw).trim().to_string();
            if !text.is_empty() {
                lines.push(text);
            }
        }
        for line in lines {
            self.add_author_line(line);
        }
    }

    fn split_authors(raw: &str) -> Vec<String> {
        let mut normalized = raw.replace("\\and", "\n\n");
        normalized = normalized.replace("\\\\", "\n");
        let mut authors = Vec::new();
        for block in normalized.split("\n\n") {
            let mut name = None;
            for line in block.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                name = Some(trimmed.to_string());
                break;
            }
            if let Some(mut name) = name {
                if name.starts_with('{') && name.ends_with('}') && name.len() > 1 {
                    name = name[1..name.len() - 1].trim().to_string();
                }
                if !name.is_empty() {
                    authors.push(name);
                }
            }
        }
        authors
    }

    fn parse_author_blocks_with_lines(raw: &str) -> Vec<AuthorBlock> {
        let mut normalized = raw.replace("\\and", "\n\n");
        normalized = normalized.replace("\\\\", "\n");
        let mut blocks = Vec::new();
        for block in normalized.split("\n\n") {
            let mut lines: Vec<String> = Vec::new();
            for line in block.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let mut cleaned = trimmed.to_string();
                if cleaned.starts_with('{') && cleaned.ends_with('}') && cleaned.len() > 1 {
                    cleaned = cleaned[1..cleaned.len() - 1].trim().to_string();
                }
                cleaned = cleaned.replace("\\@", "@");
                lines.push(cleaned);
            }
            if lines.is_empty() {
                continue;
            }
            let name = lines.first().cloned();
            let mut rest: Vec<String> = Vec::new();
            let mut email: Option<String> = None;
            for line in lines.iter().skip(1) {
                let trimmed = line.trim();
                if trimmed.contains('@') && !trimmed.contains(' ') {
                    email = Some(trimmed.to_string());
                    continue;
                }
                rest.push(trimmed.to_string());
            }
            let mut block_out = AuthorBlock::default();
            block_out.name = name;
            block_out.lines = rest;
            block_out.email = email;
            blocks.push(block_out);
        }
        blocks
    }

    fn parse_cvpr_author_groups(&self, raw: &str) -> Option<(Vec<String>, Vec<String>)> {
        let mut normalized = raw.replace("\\and", "\n\n");
        normalized = normalized.replace("\\\\", "\n");
        let mut lines: Vec<String> = Vec::new();
        for line in normalized.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut cleaned = trimmed.to_string();
            if cleaned.starts_with('{') && cleaned.ends_with('}') && cleaned.len() > 1 {
                cleaned = cleaned[1..cleaned.len() - 1].trim().to_string();
            }
            cleaned = cleaned.replace("\\@", "@");
            if let Some(idx) = cleaned.find("#link(") {
                cleaned = cleaned[..idx].trim().to_string();
            }
            if let Some(idx) = cleaned.find("http") {
                if idx == 0 {
                    continue;
                }
                cleaned = cleaned[..idx].trim().to_string();
            }
            if cleaned.is_empty() {
                continue;
            }
            lines.push(cleaned);
        }
        if lines.len() < 2 {
            return None;
        }

        let mut affl_lines: Vec<String> = Vec::new();
        while let Some(last) = lines.last() {
            let cleaned = Self::normalize_author_field_line(last);
            if cleaned.is_empty() {
                lines.pop();
                continue;
            }
            let lower = cleaned.to_ascii_lowercase();
            let is_affl_tail = Self::looks_like_organization(&cleaned)
                || Self::looks_like_department(&cleaned)
                || lower.contains("city")
                || lower.contains("state")
                || lower.contains("country")
                || lower.contains("province");
            if is_affl_tail {
                affl_lines.push(cleaned);
                lines.pop();
            } else {
                break;
            }
        }
        if affl_lines.is_empty() {
            return None;
        }
        affl_lines.reverse();

        let mut names: Vec<String> = Vec::new();
        for line in lines {
            let cleaned = Self::normalize_author_field_line(&line);
            for part in cleaned.split(',') {
                let name = part.trim();
                if name.is_empty() {
                    continue;
                }
                names.push(name.to_string());
            }
        }
        if names.is_empty() {
            return None;
        }

        Some((names, affl_lines))
    }

    fn normalize_author_field_line(line: &str) -> String {
        let mut cleaned = line.trim().to_string();
        if cleaned.starts_with('{') && cleaned.ends_with('}') && cleaned.len() > 1 {
            cleaned = cleaned[1..cleaned.len() - 1].trim().to_string();
        }
        cleaned = cleaned.replace("\\@", "@");
        cleaned = Self::strip_typst_markup(&cleaned);
        cleaned = Self::strip_typst_links(&cleaned);

        let trimmed = cleaned.trim();
        for prefix in ["#emph[", "#strong[", "#smallcaps[", "#underline["] {
            if trimmed.starts_with(prefix) && trimmed.ends_with(']') {
                return trimmed[prefix.len()..trimmed.len() - 1].trim().to_string();
            }
        }

        if trimmed.starts_with("#link(") && trimmed.ends_with(']') {
            if let Some(start) = trimmed.rfind('[') {
                let inner = &trimmed[start + 1..trimmed.len() - 1];
                if !inner.trim().is_empty() {
                    return inner.trim().to_string();
                }
            }
        }

        if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 1 {
            return trimmed[1..trimmed.len() - 1].trim().to_string();
        }

        trimmed.to_string()
    }

    fn strip_typst_markup(text: &str) -> String {
        let prefixes = ["#emph[", "#strong[", "#smallcaps[", "#underline["];
        let mut out = String::new();
        let mut i = 0usize;
        let bytes = text.as_bytes();
        while i < bytes.len() {
            let mut matched = None;
            for prefix in prefixes {
                if text[i..].starts_with(prefix) {
                    matched = Some(prefix.len());
                    break;
                }
            }
            if let Some(len) = matched {
                i += len;
                let mut depth = 1i32;
                while i < bytes.len() {
                    let ch = text[i..].chars().next().unwrap();
                    let ch_len = ch.len_utf8();
                    if ch == '[' {
                        depth += 1;
                        out.push(ch);
                    } else if ch == ']' {
                        depth -= 1;
                        if depth == 0 {
                            i += ch_len;
                            break;
                        }
                        out.push(ch);
                    } else {
                        out.push(ch);
                    }
                    i += ch_len;
                }
                continue;
            }
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn strip_typst_links(text: &str) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        let bytes = text.as_bytes();
        while i < bytes.len() {
            if text[i..].starts_with("#link(") {
                i += "#link(".len();
                let mut depth = 1i32;
                while i < bytes.len() && depth > 0 {
                    let ch = text[i..].chars().next().unwrap();
                    let ch_len = ch.len_utf8();
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            i += ch_len;
                            break;
                        }
                    }
                    i += ch_len;
                }
                while i < bytes.len() && text[i..].chars().next().unwrap().is_whitespace() {
                    i += text[i..].chars().next().unwrap().len_utf8();
                }
                if i < bytes.len() && text[i..].starts_with('[') {
                    i += 1;
                    let mut depth = 1i32;
                    while i < bytes.len() && depth > 0 {
                        let ch = text[i..].chars().next().unwrap();
                        let ch_len = ch.len_utf8();
                        if ch == '[' {
                            depth += 1;
                            out.push(ch);
                        } else if ch == ']' {
                            depth -= 1;
                            if depth == 0 {
                                i += ch_len;
                                break;
                            }
                            out.push(ch);
                        } else {
                            out.push(ch);
                        }
                        i += ch_len;
                    }
                }
                continue;
            }
            if text[i..].starts_with("#image(") {
                i += "#image(".len();
                let mut depth = 1i32;
                while i < bytes.len() && depth > 0 {
                    let ch = text[i..].chars().next().unwrap();
                    let ch_len = ch.len_utf8();
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            i += ch_len;
                            break;
                        }
                    }
                    i += ch_len;
                }
                continue;
            }
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }

    fn extract_email_from_line(line: &str) -> Option<String> {
        if let Some(idx) = line.find("mailto:") {
            let start = idx + "mailto:".len();
            let mut end = start;
            for ch in line[start..].chars() {
                if ch == '"' || ch == ']' || ch == ')' || ch.is_whitespace() {
                    break;
                }
                end += ch.len_utf8();
            }
            let candidate = line[start..end].trim();
            if candidate.contains('@') && candidate.contains('.') {
                return Some(candidate.to_string());
            }
        }

        let mut token = String::new();
        let mut best: Option<String> = None;
        for ch in line.chars() {
            if ch.is_ascii_alphanumeric() || "@._%+-".contains(ch) {
                token.push(ch);
            } else {
                if token.contains('@') && token.contains('.') {
                    best = Some(token.clone());
                    break;
                }
                token.clear();
            }
        }
        if best.is_none() && token.contains('@') && token.contains('.') {
            best = Some(token);
        }
        best
    }

    fn looks_like_department(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        lower.contains("department")
            || lower.contains("dept.")
            || lower.starts_with("dept ")
            || lower.contains("school of")
            || lower.contains("faculty of")
            || lower.contains("division of")
    }

    fn looks_like_organization(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        lower.contains("university")
            || lower.contains("institute")
            || lower.contains("laboratory")
            || lower.contains("laboratories")
            || lower.contains("lab")
            || lower.contains("college")
            || lower.contains("academy")
            || lower.contains("center")
            || lower.contains("centre")
            || lower.contains("corporation")
            || lower.contains("company")
            || lower.contains("inc.")
            || lower.contains("corp.")
            || lower.contains("ltd")
    }

    fn looks_like_location(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        line.contains(',')
            || lower.contains("city")
            || lower.contains("state")
            || lower.contains("country")
            || lower.contains("province")
    }

    fn split_name_affiliation(line: &str) -> (String, Option<String>) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return (String::new(), None);
        }
        let lower = trimmed.to_ascii_lowercase();
        let keywords = [
            "department",
            "dept.",
            "school of",
            "faculty of",
            "college of",
            "institute",
            "university",
            "laboratory",
            "lab",
        ];
        let mut split_at: Option<usize> = None;
        for kw in keywords {
            if let Some(pos) = lower.find(kw) {
                split_at = Some(split_at.map_or(pos, |prev| prev.min(pos)));
            }
        }
        if let Some(pos) = split_at {
            let name = trimmed[..pos].trim();
            let aff = trimmed[pos..].trim();
            if !name.is_empty() && !aff.is_empty() {
                return (name.to_string(), Some(aff.to_string()));
            }
        }
        (trimmed.to_string(), None)
    }

    fn render_ieee_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let mut out = String::new();
        out.push_str("#show: ieee.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let mut blocks = Vec::new();
        if let Some(raw) = author {
            blocks = Self::parse_author_blocks_with_lines(raw);
        }
        if blocks.is_empty() {
            blocks = self.collect_author_blocks();
        }
        if !blocks.is_empty() {
            out.push_str("  authors: (\n");
            for block in blocks {
                let raw_name = block.name.as_deref().unwrap_or("").trim();
                let raw_name = Self::strip_typst_markup(raw_name);
                let (mut name, name_aff) = Self::split_name_affiliation(&raw_name);
                if name.is_empty() {
                    continue;
                }
                let mut department: Option<String> = None;
                let mut organization: Option<String> = None;
                let mut location_parts: Vec<String> = Vec::new();
                let mut email = block.email.clone();

                let mut lines = Vec::new();
                if let Some(line) = name_aff.as_deref() {
                    lines.push(line.to_string());
                }
                lines.extend(self.collect_affiliation_lines(&block));

                if !lines.is_empty() {
                    let mut merged: Vec<String> = Vec::new();
                    let mut idx = 0usize;
                    while idx < lines.len() {
                        let mut line = lines[idx].trim().to_string();
                        while idx + 1 < lines.len() {
                            let lower = line.to_ascii_lowercase();
                            let next = lines[idx + 1].trim();
                            if next.is_empty() {
                                idx += 1;
                                continue;
                            }
                            if line.ends_with('-') {
                                line.pop();
                                line.push_str(next);
                                idx += 1;
                                continue;
                            }
                            if lower.ends_with("and") || lower.ends_with("of") {
                                line.push(' ');
                                line.push_str(next);
                                idx += 1;
                                continue;
                            }
                            break;
                        }
                        if !line.is_empty() {
                            merged.push(line);
                        }
                        idx += 1;
                    }
                    lines = merged;
                }

                let mut prefer_org = false;
                if name_aff.is_none() && !lines.is_empty() {
                    let tokens: Vec<&str> = name.split_whitespace().collect();
                    if tokens.len() >= 4 {
                        let split = if tokens.len() > 5 { 3 } else { 2 };
                        let new_name = tokens[..split].join(" ");
                        let aff = tokens[split..].join(" ");
                        if !new_name.trim().is_empty() && !aff.trim().is_empty() {
                            name = new_name;
                            lines.insert(0, aff);
                            prefer_org = true;
                        }
                    }
                }

                let mut name_parts: Vec<String> = Vec::new();
                for raw_line in lines {
                    let mut cleaned = Self::normalize_author_field_line(&raw_line);
                    if cleaned.is_empty() {
                        continue;
                    }
                    let mut lower = cleaned.to_ascii_lowercase();
                    if lower.starts_with("and ") {
                        let rest = cleaned[4..].trim();
                        let words: Vec<&str> = rest.split_whitespace().collect();
                        if words.len() >= 3 {
                            name_parts.push(format!("and {} {}", words[0], words[1]));
                            cleaned = words[2..].join(" ");
                            lower = cleaned.to_ascii_lowercase();
                        } else if !rest.is_empty() {
                            name_parts.push(format!("and {}", rest));
                            continue;
                        }
                    }
                    if lower.starts_with("email:")
                        || lower.starts_with("telephone:")
                        || lower.starts_with("tel:")
                        || lower.starts_with("fax:")
                    {
                        if email.is_none() {
                            if let Some(found) = Self::extract_email_from_line(&cleaned) {
                                email = Some(found);
                            }
                        }
                        continue;
                    }
                    if email.is_none() {
                        if let Some(found) = Self::extract_email_from_line(&cleaned) {
                            email = Some(found);
                            continue;
                        }
                    }

                    if department.is_none() && Self::looks_like_department(&cleaned) {
                        department = Some(cleaned);
                        continue;
                    }
                    if organization.is_none() && Self::looks_like_organization(&cleaned) {
                        organization = Some(cleaned);
                        continue;
                    }
                    if Self::looks_like_location(&cleaned) {
                        location_parts.push(cleaned);
                        continue;
                    }

                    if prefer_org && organization.is_none() {
                        organization = Some(cleaned);
                    } else if department.is_none() {
                        department = Some(cleaned);
                    } else if organization.is_none() {
                        organization = Some(cleaned);
                    } else {
                        location_parts.push(cleaned);
                    }
                }

                if !name_parts.is_empty() {
                    let mut combined = String::new();
                    combined.push_str(&name);
                    for part in name_parts {
                        combined.push(' ');
                        combined.push_str(&part);
                    }
                    name = combined.trim().to_string();
                }

                let location = if !location_parts.is_empty() {
                    Some(location_parts.join(", "))
                } else {
                    None
                };

                out.push_str("    (\n");
                let escaped = super::utils::escape_typst_string(&name);
                let _ = writeln!(out, "      name: \"{}\",", escaped);
                if let Some(department) = department.as_deref() {
                    let escaped = super::utils::escape_typst_string(department);
                    let _ = writeln!(out, "      department: \"{}\",", escaped);
                }
                if let Some(organization) = organization.as_deref() {
                    let escaped = super::utils::escape_typst_string(organization);
                    let _ = writeln!(out, "      organization: \"{}\",", escaped);
                }
                if let Some(location) = location.as_deref() {
                    let escaped = super::utils::escape_typst_string(location);
                    let _ = writeln!(out, "      location: \"{}\",", escaped);
                }
                if let Some(email) = email.as_deref() {
                    let escaped = super::utils::escape_typst_string(email);
                    let _ = writeln!(out, "      email: \"{}\",", escaped);
                }
                out.push_str("    ),\n");
            }
            out.push_str("  ),\n");
        }
        if let Some(abs) = abstract_text {
            let _ = writeln!(out, "  abstract: [{}],", abs.trim());
        }
        if !keywords.is_empty() {
            out.push_str("  index-terms: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        out.push_str(")\n\n");
        out
    }

    fn collect_author_blocks(&self) -> Vec<AuthorBlock> {
        if !self.state.author_blocks.is_empty() {
            return self.state.author_blocks.clone();
        }
        let Some(author) = self.state.author.as_deref() else {
            return Vec::new();
        };
        Self::split_authors(author)
            .into_iter()
            .map(|name| AuthorBlock {
                name: Some(name),
                ..Default::default()
            })
            .collect()
    }

    fn collect_affiliation_lines(&self, block: &AuthorBlock) -> Vec<String> {
        let mut lines = Vec::new();
        for line in &block.lines {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
        if !block.affiliation_keys.is_empty() {
            for key in &block.affiliation_keys {
                if let Some(text) = self.state.affiliation_map.get(key) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.to_string());
                    }
                }
            }
        }
        lines
    }

    fn render_acm_show_rule(
        &self,
        title: Option<&str>,
        _abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let mut out = String::new();
        out.push_str("#show: acmart.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }

        let blocks = self.collect_author_blocks();
        if !blocks.is_empty() {
            out.push_str("  authors: (\n");
            for block in blocks {
                let name = block.name.as_deref().unwrap_or("").trim();
                if name.is_empty() {
                    continue;
                }
                out.push_str("    (\n");
                let escaped_name = super::utils::escape_typst_text(name);
                let _ = writeln!(out, "      name: [{}],", escaped_name);
                // Always include email field (clean-acmart template requires it)
                if let Some(email) = block.email.as_deref() {
                    if !email.trim().is_empty() {
                        let escaped = super::utils::escape_typst_text(email.trim());
                        let _ = writeln!(out, "      email: [{}],", escaped);
                    } else {
                        out.push_str("      email: [],\n");
                    }
                } else {
                    out.push_str("      email: [],\n");
                }
                let aff_lines = self.collect_affiliation_lines(&block);
                for (idx, line) in aff_lines.iter().enumerate() {
                    let escaped = super::utils::escape_typst_text(line);
                    let _ = writeln!(out, "      aff{}: [{}],", idx + 1, escaped);
                }
                out.push_str("    ),\n");
            }
            out.push_str("  ),\n");
        }

        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }

        out.push_str(")\n\n");
        out
    }

    fn render_lncs_show_rule(
        &self,
        title: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let mut out = String::new();

        let blocks = self.collect_author_blocks();
        let mut institute_defs: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();
        let mut institute_keys: HashMap<String, String> = HashMap::new();
        let mut authors_out = Vec::new();

        for block in blocks {
            let name = block.name.as_deref().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let mut insts = Vec::new();
            let aff_lines = self.collect_affiliation_lines(&block);
            if !aff_lines.is_empty() {
                let inst_name = aff_lines[0].clone();
                let addr = if aff_lines.len() > 1 {
                    Some(aff_lines[1..].join(", "))
                } else {
                    None
                };
                let email = block
                    .email
                    .as_deref()
                    .map(str::to_string)
                    .filter(|e| !e.trim().is_empty());
                let key = format!(
                    "{}|{}|{}",
                    inst_name,
                    addr.clone().unwrap_or_default(),
                    email.clone().unwrap_or_default()
                );
                let var = if let Some(existing) = institute_keys.get(&key) {
                    existing.clone()
                } else {
                    let var = format!("inst_{}", institute_defs.len() + 1);
                    institute_defs.push((var.clone(), inst_name, addr, email));
                    institute_keys.insert(key, var.clone());
                    var
                };
                insts.push(var);
            }

            let escaped_name = super::utils::escape_typst_string(name);
            if insts.is_empty() {
                authors_out.push(format!("author(\"{}\")", escaped_name));
            } else {
                let inst_list = insts.join(", ");
                authors_out.push(format!(
                    "author(\"{}\", insts: ({}))",
                    escaped_name, inst_list
                ));
            }
        }

        for (var, name, addr, email) in &institute_defs {
            let escaped_name = super::utils::escape_typst_string(name);
            out.push_str(&format!("#let {} = institute(\"{}\"", var, escaped_name));
            if let Some(addr) = addr {
                let escaped = super::utils::escape_typst_string(addr);
                out.push_str(&format!(", addr: \"{}\"", escaped));
            }
            if let Some(email) = email {
                let escaped = super::utils::escape_typst_string(email);
                out.push_str(&format!(", email: \"{}\"", escaped));
            }
            out.push_str(")\n");
        }
        if !institute_defs.is_empty() {
            out.push('\n');
        }

        out.push_str("#show: lncs.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        if !authors_out.is_empty() {
            out.push_str("  authors: (\n");
            for author in authors_out {
                let _ = writeln!(out, "    {},", author);
            }
            out.push_str("  ),\n");
        }
        if let Some(abs) = abstract_text {
            let _ = writeln!(out, "  abstract: [{}],", abs.trim());
        }
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        out.push_str(")\n\n");
        out
    }

    fn collect_author_blocks_from_arg(&self, author: Option<&str>) -> Vec<AuthorBlock> {
        let mut blocks = Vec::new();
        if let Some(raw) = author {
            blocks = Self::parse_author_blocks_with_lines(raw);
        }
        if blocks.is_empty() {
            blocks = self.collect_author_blocks();
        }
        blocks
    }

    fn extract_author_fields(
        &self,
        block: &AuthorBlock,
    ) -> (
        Option<String>,
        Option<String>,
        Vec<String>,
        Option<String>,
        Vec<String>,
    ) {
        let mut department: Option<String> = None;
        let mut institution: Option<String> = None;
        let mut location_parts: Vec<String> = Vec::new();
        let mut email = block.email.clone();
        let mut extra_parts: Vec<String> = Vec::new();

        for raw_line in self.collect_affiliation_lines(block) {
            let cleaned = Self::normalize_author_field_line(&raw_line);
            if cleaned.is_empty() {
                continue;
            }
            let lower = cleaned.to_ascii_lowercase();
            if lower.starts_with("email:") {
                if email.is_none() {
                    if let Some(found) = Self::extract_email_from_line(&cleaned) {
                        email = Some(found);
                    }
                }
                continue;
            }
            if email.is_none() {
                if let Some(found) = Self::extract_email_from_line(&cleaned) {
                    email = Some(found);
                    continue;
                }
            }
            if department.is_none() && Self::looks_like_department(&cleaned) {
                department = Some(cleaned);
                continue;
            }
            if institution.is_none() && Self::looks_like_organization(&cleaned) {
                institution = Some(cleaned);
                continue;
            }
            if Self::looks_like_location(&cleaned) {
                location_parts.push(cleaned);
                continue;
            }
            if institution.is_none() {
                institution = Some(cleaned);
            } else {
                extra_parts.push(cleaned);
            }
        }

        (department, institution, location_parts, email, extra_parts)
    }

    fn build_author_affl_entries(
        &self,
        blocks: &[AuthorBlock],
    ) -> (Vec<String>, Vec<String>) {
        let mut author_entries: Vec<String> = Vec::new();
        let mut affl_entries: Vec<String> = Vec::new();

        for (idx, block) in blocks.iter().enumerate() {
            let name = block.name.as_deref().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let (department, institution, mut location_parts, email, extra_parts) =
                self.extract_author_fields(block);
            location_parts.extend(extra_parts);

            // Build affiliation only if there's actual content
            let affl_key = format!("affl{}", idx + 1);
            let mut affl_content = String::new();
            if let Some(department) = department.as_deref() {
                let escaped = super::utils::escape_typst_string(department);
                let _ = write!(affl_content, "department: \"{}\", ", escaped);
            }
            if let Some(institution) = institution.as_deref() {
                let escaped = super::utils::escape_typst_string(institution);
                let _ = write!(affl_content, "institution: \"{}\", ", escaped);
            }
            if !location_parts.is_empty() {
                let escaped = super::utils::escape_typst_string(&location_parts.join(", "));
                let _ = write!(affl_content, "location: \"{}\", ", escaped);
            }
            let has_affl = !affl_content.is_empty();
            if has_affl {
                if affl_content.ends_with(", ") {
                    affl_content.truncate(affl_content.len() - 2);
                }
                let affl = format!("{}: ({})", affl_key, affl_content);
                affl_entries.push(affl);
            }

            let escaped_name = super::utils::escape_typst_string(name);
            let mut author_entry = String::new();
            author_entry.push_str("(name: \"");
            author_entry.push_str(&escaped_name);
            if has_affl {
                author_entry.push_str("\", affl: (\"");
                author_entry.push_str(&affl_key);
                author_entry.push_str("\",)");
            } else {
                // Include empty affl tuple for template compatibility
                author_entry.push_str("\", affl: ()");
            }
            if let Some(email) = email.as_deref() {
                let escaped = super::utils::escape_typst_string(email.trim());
                if !escaped.is_empty() {
                    author_entry.push_str(", email: \"");
                    author_entry.push_str(&escaped);
                    author_entry.push('"');
                }
            }
            author_entry.push(')');
            author_entries.push(author_entry);
        }

        (author_entries, affl_entries)
    }

    fn format_affl_tuple(&self, author_entries: &[String], affl_entries: &[String]) -> String {
        let mut authors_value = String::new();
        authors_value.push_str("(\n    (");
        if !author_entries.is_empty() {
            authors_value.push('\n');
            for entry in author_entries {
                authors_value.push_str("      ");
                authors_value.push_str(entry);
                authors_value.push_str(",\n");
            }
            authors_value.push_str("    ),\n    (\n");
            for entry in affl_entries {
                authors_value.push_str("      ");
                authors_value.push_str(entry);
                authors_value.push_str(",\n");
            }
            authors_value.push_str("    )\n  )");
        } else {
            authors_value.push_str("), ())");
        }
        authors_value
    }

    fn template_year_from_package(&self) -> Option<u32> {
        let package = self.state.template_package.as_deref()?;
        let digits: String = package.chars().filter(|ch| ch.is_ascii_digit()).collect();
        if digits.len() < 4 {
            return None;
        }
        digits[0..4].parse::<u32>().ok()
    }

    fn ifdefined_value(&self, name: &str) -> Option<bool> {
        match name {
            "aaaianonymous" => Some(self.state.aaai_anonymous),
            _ => None,
        }
    }

    fn strip_ifdefined_blocks(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut i = 0usize;
        let bytes = input.as_bytes();
        while let Some(pos) = input[i..].find("\\ifdefined") {
            let start = i + pos;
            out.push_str(&input[i..start]);
            let mut j = start + "\\ifdefined".len();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'\\' {
                j += 1;
            }
            let name_start = j;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'@')
            {
                j += 1;
            }
            if name_start == j {
                out.push_str("\\ifdefined");
                i = start + "\\ifdefined".len();
                continue;
            }
            let name = input
                .get(name_start..j)
                .unwrap_or("")
                .to_ascii_lowercase();
            let cond = match self.ifdefined_value(name.trim()) {
                Some(value) => value,
                None => {
                    out.push_str(input.get(start..j).unwrap_or(""));
                    i = j;
                    continue;
                }
            };

            let mut depth = 1i32;
            let mut k = j;
            let mut else_pos: Option<usize> = None;
            let mut end_pos: Option<usize> = None;
            while k < bytes.len() {
                if bytes.get(k..).map(|s| s.starts_with(b"\\ifdefined")) == Some(true) {
                    depth += 1;
                    k += "\\ifdefined".len();
                    continue;
                }
                if bytes.get(k..).map(|s| s.starts_with(b"\\fi")) == Some(true) {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = Some(k);
                        break;
                    }
                    k += "\\fi".len();
                    continue;
                }
                if depth == 1
                    && else_pos.is_none()
                    && bytes.get(k..).map(|s| s.starts_with(b"\\else")) == Some(true)
                {
                    else_pos = Some(k);
                    k += "\\else".len();
                    continue;
                }
                k += 1;
            }

            let end = match end_pos {
                Some(pos) => pos,
                None => {
                    out.push_str(&input[start..]);
                    return out;
                }
            };

            let then_start = j;
            let then_end = else_pos.unwrap_or(end);
            let else_start = else_pos.map(|p| p + "\\else".len()).unwrap_or(end);
            let chosen = if cond {
                input.get(then_start..then_end).unwrap_or("")
            } else {
                input.get(else_start..end).unwrap_or("")
            };
            out.push_str(chosen);
            i = end + "\\fi".len();
        }
        out.push_str(&input[i..]);
        out
    }

    fn render_cvpr_block_open(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
    ) -> String {
        let mut out = String::new();
        out.push_str("#import \"/cvpr.typ\": cvpr\n\n");

        let mut author_entries: Vec<String> = Vec::new();
        let mut affl_entries: Vec<String> = Vec::new();
        let mut extras_line: Option<String> = None;

        let mut used_cvpr_group = false;
        if let Some(raw_author) = author {
            if let Some((names, affl_lines)) = self.parse_cvpr_author_groups(raw_author) {
                let mut affl_block = AuthorBlock::default();
                affl_block.lines = affl_lines;
                let (department, institution, mut location_parts, _, extra_parts) =
                    self.extract_author_fields(&affl_block);
                location_parts.extend(extra_parts);

                let affl_key = "affl1".to_string();
                let mut affl = String::new();
                affl.push_str(&format!("{}: (", affl_key));
                if let Some(department) = department.as_deref() {
                    let escaped = super::utils::escape_typst_string(department);
                    let _ = write!(affl, "department: \"{}\", ", escaped);
                }
                if let Some(institution) = institution.as_deref() {
                    let escaped = super::utils::escape_typst_string(institution);
                    let _ = write!(affl, "institution: \"{}\", ", escaped);
                }
                if !location_parts.is_empty() {
                    let escaped = super::utils::escape_typst_string(&location_parts.join(", "));
                    let _ = write!(affl, "location: \"{}\", ", escaped);
                }
                if affl.ends_with(", ") {
                    affl.truncate(affl.len() - 2);
                }
                affl.push_str(")");
                affl_entries.push(affl);

                for name in names {
                    let escaped_name = super::utils::escape_typst_string(&name);
                    let mut author_entry = String::new();
                    author_entry.push_str("(name: \"");
                    author_entry.push_str(&escaped_name);
                    author_entry.push_str("\", affl: (\"");
                    author_entry.push_str(&affl_key);
                    author_entry.push_str("\",))");
                    author_entries.push(author_entry);
                }
                used_cvpr_group = !author_entries.is_empty();
            }
        }

        if let Some(raw_author) = author {
            let buttons = self.extract_linkbuttons(raw_author);
            if !buttons.is_empty() {
                let mut extras = String::new();
                for (idx, (url, label)) in buttons.into_iter().enumerate() {
                    if idx > 0 {
                        extras.push_str(" #h(8pt) ");
                    }
                    let url_escaped = super::utils::escape_typst_string(&url);
                    let label_escaped = super::utils::escape_typst_text(&label);
                    let _ = write!(
                        extras,
                        "#link(\"{}\")[#strong[{}]]",
                        url_escaped, label_escaped
                    );
                }
                extras_line = Some(extras);
            }
        }

        if !used_cvpr_group {
            let mut blocks = Vec::new();
            if let Some(raw_author) = author {
                blocks = Self::parse_author_blocks_with_lines(raw_author);
            }
            if blocks.is_empty() {
                blocks = self.collect_author_blocks();
            }

            for (idx, block) in blocks.iter().enumerate() {
                let name = block.name.as_deref().unwrap_or("").trim();
                if name.is_empty() {
                    continue;
                }
                let mut department: Option<String> = None;
                let mut institution: Option<String> = None;
                let mut location_parts: Vec<String> = Vec::new();
                let mut email = block.email.clone();

                for raw_line in self.collect_affiliation_lines(block) {
                    let cleaned = Self::normalize_author_field_line(&raw_line);
                    if cleaned.is_empty() {
                        continue;
                    }
                    let lower = cleaned.to_ascii_lowercase();
                    if lower.starts_with("email:") {
                        if email.is_none() {
                            if let Some(found) = Self::extract_email_from_line(&cleaned) {
                                email = Some(found);
                            }
                        }
                        continue;
                    }
                    if email.is_none() {
                        if let Some(found) = Self::extract_email_from_line(&cleaned) {
                            email = Some(found);
                            continue;
                        }
                    }
                    if department.is_none() && Self::looks_like_department(&cleaned) {
                        department = Some(cleaned);
                        continue;
                    }
                    if institution.is_none() && Self::looks_like_organization(&cleaned) {
                        institution = Some(cleaned);
                        continue;
                    }
                    if Self::looks_like_location(&cleaned) {
                        location_parts.push(cleaned);
                        continue;
                    }
                    if institution.is_none() {
                        institution = Some(cleaned);
                    } else {
                        location_parts.push(cleaned);
                    }
                }

                let affl_key = format!("affl{}", idx + 1);
                let mut affl = String::new();
                affl.push_str(&format!("{}: (", affl_key));
                if let Some(department) = department.as_deref() {
                    let escaped = super::utils::escape_typst_string(department);
                    let _ = write!(affl, "department: \"{}\", ", escaped);
                }
                if let Some(institution) = institution.as_deref() {
                    let escaped = super::utils::escape_typst_string(institution);
                    let _ = write!(affl, "institution: \"{}\", ", escaped);
                }
                if !location_parts.is_empty() {
                    let escaped = super::utils::escape_typst_string(&location_parts.join(", "));
                    let _ = write!(affl, "location: \"{}\", ", escaped);
                }
                if affl.ends_with(", ") {
                    affl.truncate(affl.len() - 2);
                }
                affl.push_str(")");
                affl_entries.push(affl);

                let escaped_name = super::utils::escape_typst_string(name);
                let mut author_entry = String::new();
                author_entry.push_str("(name: \"");
                author_entry.push_str(&escaped_name);
                author_entry.push_str("\", affl: (\"");
                author_entry.push_str(&affl_key);
                author_entry.push_str("\",)");
                if let Some(email) = email.as_deref() {
                    let escaped = super::utils::escape_typst_string(email.trim());
                    if !escaped.is_empty() {
                        author_entry.push_str(", email: \"");
                        author_entry.push_str(&escaped);
                        author_entry.push('"');
                    }
                }
                author_entry.push(')');
                author_entries.push(author_entry);
            }
        }

        let mut authors_value = String::new();
        authors_value.push_str("(\n    (");
        if !author_entries.is_empty() {
            authors_value.push('\n');
            for entry in &author_entries {
                authors_value.push_str("      ");
                authors_value.push_str(entry);
                authors_value.push_str(",\n");
            }
            authors_value.push_str("    ),\n    (\n");
            for entry in &affl_entries {
                authors_value.push_str("      ");
                authors_value.push_str(entry);
                authors_value.push_str(",\n");
            }
            authors_value.push_str("    )\n  )");
        } else {
            authors_value.push_str("), ())");
        }

        out.push_str("#cvpr(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let accepted = if self.state.cvpr_review {
            "false"
        } else if self.state.cvpr_final {
            "true"
        } else {
            "none"
        };
        let _ = writeln!(out, "  accepted: {},", accepted);
        if let Some(id) = self.state.cvpr_paper_id.as_deref() {
            let _ = writeln!(out, "  id: \"{}\",", id);
        }
        if let Some(year) = self.state.cvpr_conf_year.as_deref() {
            let _ = writeln!(out, "  aux: (conf-year: \"{}\"),", year);
        }
        let _ = writeln!(out, "  authors: {},", authors_value);
        if let Some(extras) = extras_line.as_deref() {
            let _ = writeln!(out, "  extras: [{}],", extras);
        }
        if let Some(abs) = abstract_text {
            let cleaned = strip_abstract_label(abs);
            let _ = writeln!(out, "  abstract: [{}],", cleaned.trim());
        }
        out.push_str(")[\n\n");

        out
    }

    fn extract_linkbuttons(&self, raw: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut pos = 0usize;
        while let Some(idx) = raw[pos..].find("\\linkbutton") {
            let start = pos + idx + "\\linkbutton".len();
            let (url, next1) = extract_braced_arg_at(raw, start);
            let (icon, next2) = match next1 {
                Some(next) => extract_braced_arg_at(raw, next),
                None => (None, None),
            };
            let (label, next3) = match next2 {
                Some(next) => extract_braced_arg_at(raw, next),
                None => (None, None),
            };
            if let (Some(url), Some(label)) = (url, label) {
                let clean_label = label
                    .replace('\\', "")
                    .replace('{', "")
                    .replace('}', "")
                    .trim()
                    .to_string();
                out.push((url.trim().to_string(), clean_label));
            }
            let _ = icon;
            let next = next3.or(next2).or(next1).unwrap_or(start + 1);
            pos = next;
        }
        out
    }

    fn render_iclr_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let blocks = self.collect_author_blocks_from_arg(author);
        let mut author_entries: Vec<String> = Vec::new();

        for block in blocks {
            let name = block.name.as_deref().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let (department, institution, location_parts, email, extra_parts) =
                self.extract_author_fields(&block);
            let mut affl_parts: Vec<String> = Vec::new();
            if let Some(department) = department {
                affl_parts.push(department);
            }
            if let Some(institution) = institution {
                affl_parts.push(institution);
            }
            affl_parts.extend(extra_parts);

            let affl_text = if !affl_parts.is_empty() {
                let parts: Vec<String> = affl_parts
                    .iter()
                    .map(|part| super::utils::escape_typst_text(part))
                    .collect();
                Some(parts.join(" \\\\ "))
            } else {
                None
            };
            let address_text = if !location_parts.is_empty() {
                Some(super::utils::escape_typst_text(&location_parts.join(", ")))
            } else {
                None
            };

            let mut entry = String::new();
            let escaped_name = super::utils::escape_typst_text(name);
            entry.push_str("(names: ([");
            entry.push_str(&escaped_name);
            entry.push_str("],)");
            if let Some(affl_text) = affl_text {
                entry.push_str(", affilation: [");
                entry.push_str(&affl_text);
                entry.push(']');
            }
            if let Some(address_text) = address_text {
                entry.push_str(", address: [");
                entry.push_str(&address_text);
                entry.push(']');
            }
            if let Some(email) = email.as_deref() {
                let escaped = super::utils::escape_typst_string(email.trim());
                if !escaped.is_empty() {
                    entry.push_str(", email: \"");
                    entry.push_str(&escaped);
                    entry.push('"');
                }
            }
            entry.push(')');
            author_entries.push(entry);
        }

        let mut out = String::new();
        out.push_str("#import \"/iclr.typ\": iclr\n\n");
        out.push_str("#show: iclr.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        if !author_entries.is_empty() {
            out.push_str("  authors: (\n");
            for entry in &author_entries {
                let _ = writeln!(out, "    {},", entry);
            }
            out.push_str("  ),\n");
        }
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        let accepted = if self.state.iclr_final { "true" } else { "false" };
        let _ = writeln!(out, "  accepted: {},", accepted);
        if let Some(abs) = abstract_text {
            let cleaned = strip_abstract_label(abs);
            let _ = writeln!(out, "  abstract: [{}],", cleaned.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_icml_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let blocks = self.collect_author_blocks_from_arg(author);
        let (author_entries, affl_entries) = self.build_author_affl_entries(&blocks);
        let authors_value = self.format_affl_tuple(&author_entries, &affl_entries);

        let template = match self.template_year_from_package() {
            Some(2024) => "icml2024",
            _ => "icml2025",
        };

        let mut out = String::new();
        let _ = writeln!(out, "#import \"/{}.typ\": {}", template, template);
        out.push('\n');
        let _ = writeln!(out, "#show: {}.with(", template);
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let _ = writeln!(out, "  authors: {},", authors_value);
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        let accepted = if self.state.icml_accepted { "true" } else { "false" };
        let _ = writeln!(out, "  accepted: {},", accepted);
        if let Some(abs) = abstract_text {
            let cleaned = strip_abstract_label(abs);
            let _ = writeln!(out, "  abstract: [{}],", cleaned.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_neurips_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let blocks = self.collect_author_blocks_from_arg(author);
        let (author_entries, affl_entries) = self.build_author_affl_entries(&blocks);
        let authors_value = self.format_affl_tuple(&author_entries, &affl_entries);
        let template = match self.template_year_from_package() {
            Some(2024) => "neurips2024",
            Some(2025) => "neurips2025",
            _ => "neurips2023",
        };

        let mut out = String::new();
        let _ = writeln!(out, "#import \"/{}.typ\": {}", template, template);
        out.push('\n');
        let _ = writeln!(out, "#show: {}.with(", template);
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let accepted = if self.state.neurips_preprint {
            "none"
        } else if self.state.neurips_final {
            "true"
        } else {
            "false"
        };
        let _ = writeln!(out, "  accepted: {},", accepted);
        let _ = writeln!(out, "  authors: {},", authors_value);
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        if let Some(abs) = abstract_text {
            let cleaned = strip_abstract_label(abs);
            let _ = writeln!(out, "  abstract: [{}],", cleaned.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_jmlr_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let blocks = self.collect_author_blocks_from_arg(author);
        let (author_entries, affl_entries) = self.build_author_affl_entries(&blocks);
        let authors_value = self.format_affl_tuple(&author_entries, &affl_entries);

        let mut out = String::new();
        out.push_str("#import \"/jmlr.typ\": jmlr\n\n");
        out.push_str("#show: jmlr.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let _ = writeln!(out, "  authors: {},", authors_value);
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        if let Some(abs) = abstract_text {
            let cleaned = strip_abstract_label(abs);
            let _ = writeln!(out, "  abstract: [{}],", cleaned.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_tmlr_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let blocks = self.collect_author_blocks_from_arg(author);
        let (author_entries, affl_entries) = self.build_author_affl_entries(&blocks);
        let authors_value = self.format_affl_tuple(&author_entries, &affl_entries);

        let mut out = String::new();
        out.push_str("#import \"/tmlr.typ\": tmlr\n\n");
        out.push_str("#show: tmlr.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let accepted = if self.state.tmlr_preprint {
            "none"
        } else if self.state.tmlr_accepted {
            "true"
        } else {
            "false"
        };
        let _ = writeln!(out, "  accepted: {},", accepted);
        let _ = writeln!(out, "  authors: {},", authors_value);
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        if let Some(abs) = abstract_text {
            let _ = writeln!(out, "  abstract: [{}],", abs.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_rlj_show_rule(
        &self,
        title: Option<&str>,
        author: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let blocks = self.collect_author_blocks_from_arg(author);
        let (author_entries, affl_entries) = self.build_author_affl_entries(&blocks);
        let authors_value = self.format_affl_tuple(&author_entries, &affl_entries);

        let mut out = String::new();
        out.push_str("#import \"/rlj.typ\": rlj\n\n");
        out.push_str("#show: rlj.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        let accepted = if self.state.rlj_preprint {
            "none"
        } else if self.state.rlj_final {
            "true"
        } else {
            "false"
        };
        let _ = writeln!(out, "  accepted: {},", accepted);
        let _ = writeln!(out, "  authors: {},", authors_value);
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        if let Some(abs) = abstract_text {
            let _ = writeln!(out, "  abstract: [{}],", abs.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_elsevier_show_rule(
        &self,
        title: Option<&str>,
        abstract_text: Option<&str>,
        keywords: &[String],
    ) -> String {
        let mut out = String::new();
        let blocks = self.collect_author_blocks();
        let affiliation_key = |idx: usize| -> String {
            if idx < 26 {
                let c = (b'a' + idx as u8) as char;
                c.to_string()
            } else {
                format!("aff{}", idx + 1)
            }
        };

        let mut institutions: Vec<(String, String)> = Vec::new();
        let mut institution_keys: HashMap<String, String> = HashMap::new();
        let mut authors_out = Vec::new();

        for block in blocks {
            let name = block.name.as_deref().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let mut fields = Vec::new();
            let escaped_name = super::utils::escape_typst_text(name);
            fields.push(format!("name: [{}]", escaped_name));

            let aff_lines = self.collect_affiliation_lines(&block);
            if !aff_lines.is_empty() {
                let aff_text = aff_lines.join(", ");
                let key = if let Some(existing) = institution_keys.get(&aff_text) {
                    existing.clone()
                } else {
                    let next_key = affiliation_key(institutions.len());
                    institutions.push((next_key.clone(), aff_text.clone()));
                    institution_keys.insert(aff_text, next_key.clone());
                    next_key
                };
                let inst_list = format!("\"{}\",", key);
                fields.push(format!("institutions: ({})", inst_list));
            } else {
                fields.push("institutions: (\"\",)".to_string());
            }

            if let Some(email) = block.email.as_deref() {
                if !email.trim().is_empty() {
                    let escaped = super::utils::escape_typst_string(email.trim());
                    fields.push(format!("email: \"{}\"", escaped));
                }
            }
            fields.push("corresponding: true".to_string());

            authors_out.push(format!("({})", fields.join(", ")));
        }

        out.push_str("#show: elsevier-replica.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        if authors_out.is_empty() {
            authors_out.push("(name: \"\", institutions: (\"\",), corresponding: true)".to_string());
        }
        if !authors_out.is_empty() {
            out.push_str("  authors: (\n");
            for author in authors_out {
                let _ = writeln!(out, "    {},", author);
            }
            out.push_str("  ),\n");
        }
        if !institutions.is_empty() {
            out.push_str("  institutions: (\n");
            for (key, value) in institutions {
                let escaped = super::utils::escape_typst_text(&value);
                let _ = writeln!(out, "    \"{}\": [{}],", key, escaped);
            }
            out.push_str("  ),\n");
        }
        out.push_str("  paper-info: (extra-info: none),\n");
        if let Some(abs) = abstract_text {
            let _ = writeln!(out, "  abstract: [{}],", abs.trim());
        }
        if !keywords.is_empty() {
            out.push_str("  keywords: (");
            let mut first = true;
            for kw in keywords {
                let kw = kw.trim();
                if kw.is_empty() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let escaped = super::utils::escape_typst_string(kw);
                out.push('"');
                out.push_str(&escaped);
                out.push('"');
            }
            out.push_str("),\n");
        }
        out.push_str(")\n\n");
        out
    }

    fn render_springer_show_rule(
        &self,
        title: Option<&str>,
        abstract_text: Option<&str>,
        _keywords: &[String],
    ) -> String {
        let mut out = String::new();
        let blocks = self.collect_author_blocks();

        out.push_str("#show: template.with(\n");
        if let Some(title) = title {
            let escaped = super::utils::escape_typst_text(title);
            let _ = writeln!(out, "  title: [{}],", escaped);
        }
        if !blocks.is_empty() {
            out.push_str("  authors: (\n");
            for block in blocks {
                let name = block.name.as_deref().unwrap_or("").trim();
                if name.is_empty() {
                    continue;
                }
                let mut fields = Vec::new();
                let escaped_name = super::utils::escape_typst_string(name);
                fields.push(format!("name: \"{}\"", escaped_name));

                let aff_lines = self.collect_affiliation_lines(&block);
                if !aff_lines.is_empty() {
                    let inst = aff_lines[0].clone();
                    let escaped = super::utils::escape_typst_string(&inst);
                    fields.push(format!("institute: \"{}\"", escaped));
                    if aff_lines.len() > 1 {
                        let addr = aff_lines[1..].join(", ");
                        let escaped = super::utils::escape_typst_string(&addr);
                        fields.push(format!("address: \"{}\"", escaped));
                    }
                }
                if let Some(email) = block.email.as_deref() {
                    if !email.trim().is_empty() {
                        let escaped = super::utils::escape_typst_string(email.trim());
                        fields.push(format!("email: \"{}\"", escaped));
                    }
                }
                let _ = writeln!(out, "    ({}),", fields.join(", "));
            }
            out.push_str("  ),\n");
        }
        if let Some(abs) = abstract_text {
            let _ = writeln!(out, "  abstract: [{}],", abs.trim());
        }
        out.push_str(")\n\n");
        out
    }

    fn render_thesis_meta_block(&self) -> String {
        if self.state.thesis_meta.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        out.push_str("#block(width: 100%, inset: 1em)[\n");
        out.push_str("  #text(weight: \"bold\")[Thesis Metadata]\n");
        out.push_str("  #v(0.5em)\n");
        for (label, value) in &self.state.thesis_meta {
            let label = super::utils::escape_typst_string(label);
            let value = super::utils::escape_typst_text(value);
            let _ = writeln!(out, "  - *{}:* {}", label, value);
        }
        out.push_str("]\n\n");
        out
    }

    // ============================================================
    // Helper methods for submodules
    // ============================================================

    /// Process SI unit string
    pub fn process_si_unit(&self, input: &str) -> String {
        let mut result = input.to_string();

        for (cmd, val) in crate::siunitx::SI_UNITS.iter() {
            result = result.replace(cmd, val);
        }
        for (cmd, val) in crate::siunitx::SI_PREFIXES.iter() {
            result = result.replace(cmd, val);
        }

        result = result
            .replace("\\per", "/")
            .replace("\\squared", "²")
            .replace("\\cubed", "³")
            .replace(" ", "");

        result
    }

    /// Extract raw content from a verbatim-like environment
    pub fn extract_env_raw_content(&self, node: &SyntaxNode) -> String {
        let mut content = String::new();

        for child in node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::ItemBegin | SyntaxKind::ItemEnd => continue,
                _ => {
                    if let SyntaxElement::Token(t) = child {
                        content.push_str(t.text());
                    } else if let SyntaxElement::Node(n) = child {
                        content.push_str(&n.text().to_string());
                    }
                }
            }
        }

        content
    }

    /// Visit environment content (excluding begin/end)
    pub fn visit_env_content(&mut self, node: &SyntaxNode, output: &mut String) {
        for child in node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::ItemBegin | SyntaxKind::ItemEnd => continue,
                _ => self.visit_element(child, output),
            }
        }
    }

    fn convert_clause_argument_node(&mut self, node: &SyntaxNode) -> String {
        let pending = self.state.pending_heading.take();
        let mut output = String::new();
        for child in node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::TokenLBrace
                | SyntaxKind::TokenRBrace
                | SyntaxKind::TokenLBracket
                | SyntaxKind::TokenRBracket => continue,
                _ => self.visit_element(child, &mut output),
            }
        }
        self.state.pending_heading = pending;
        output.trim().to_string()
    }

    fn consume_pending_heading(&mut self, elem: &SyntaxElement, output: &mut String) -> bool {
        if self.state.pending_heading.is_none() {
            return false;
        }

        let kind = elem.kind();

        if kind == SyntaxKind::ClauseArgument {
            if let SyntaxElement::Node(node) = elem {
                let is_bracket = node.children().any(|c| c.kind() == SyntaxKind::ItemBracket);
                let is_curly = node.children().any(|c| c.kind() == SyntaxKind::ItemCurly);
                if is_bracket || is_curly {
                    let content = self.convert_clause_argument_node(node);
                    if let Some(pending) = self.state.pending_heading.as_mut() {
                        if !content.trim().is_empty() {
                            if is_bracket && pending.optional.is_none() {
                                pending.optional = Some(content.clone());
                            }
                            if is_curly && pending.required.is_none() {
                                pending.required = Some(content);
                            }
                        }
                        if pending.required.is_some() {
                            self.flush_pending_heading(output);
                        }
                    }
                    return true;
                }
            }
        }

        let capture_mode = self
            .state
            .pending_heading
            .as_ref()
            .map(|p| p.capture_mode)
            .unwrap_or(HeadingCaptureMode::None);

        if matches!(capture_mode, HeadingCaptureMode::Required) {
            if let SyntaxElement::Node(node) = elem {
                if let Some(cmd) = CmdItem::cast(node.clone()) {
                    if let Some(name) = cmd.name_tok() {
                        let base = name.text().trim_start_matches('\\');
                        if base == "label" {
                            if let Some(pending) = self.state.pending_heading.as_mut() {
                                let content = pending.capture_buffer.trim().to_string();
                                if !content.is_empty() && pending.required.is_none() {
                                    pending.required = Some(content);
                                }
                                pending.capture_mode = HeadingCaptureMode::None;
                                pending.capture_buffer.clear();
                                pending.implicit_open = false;
                                self.flush_pending_heading(output);
                            }
                            return false;
                        }
                    }
                }
            }
        }

        if !matches!(capture_mode, HeadingCaptureMode::None) {
            match kind {
                SyntaxKind::TokenRBracket => {
                    if let Some(pending) = self.state.pending_heading.as_mut() {
                        if matches!(pending.capture_mode, HeadingCaptureMode::Optional) {
                            pending.capture_depth = pending.capture_depth.saturating_sub(1);
                            if pending.capture_depth == 0 {
                                let content = pending.capture_buffer.trim().to_string();
                                if !content.is_empty() && pending.optional.is_none() {
                                    pending.optional = Some(content);
                                }
                                pending.capture_mode = HeadingCaptureMode::None;
                                pending.capture_buffer.clear();
                                pending.capture_mode = HeadingCaptureMode::Required;
                                pending.capture_depth = 1;
                                pending.capture_buffer.clear();
                                pending.implicit_open = true;
                            }
                            return true;
                        }
                    }
                }
                SyntaxKind::TokenRBrace => {
                    if let Some(pending) = self.state.pending_heading.as_mut() {
                        if matches!(pending.capture_mode, HeadingCaptureMode::Required) {
                            pending.capture_depth = pending.capture_depth.saturating_sub(1);
                            if pending.capture_depth == 0 {
                                let content = pending.capture_buffer.trim().to_string();
                                if !content.is_empty() && pending.required.is_none() {
                                    pending.required = Some(content);
                                }
                                pending.capture_mode = HeadingCaptureMode::None;
                                pending.capture_buffer.clear();
                                pending.implicit_open = false;
                                self.flush_pending_heading(output);
                            }
                            return true;
                        }
                    }
                }
                SyntaxKind::TokenLBracket => {
                    if let Some(pending) = self.state.pending_heading.as_mut() {
                        if matches!(pending.capture_mode, HeadingCaptureMode::Optional) {
                            if pending.implicit_open {
                                pending.implicit_open = false;
                            } else {
                                pending.capture_depth += 1;
                            }
                            return true;
                        }
                    }
                }
                SyntaxKind::TokenLBrace => {
                    if let Some(pending) = self.state.pending_heading.as_mut() {
                        if matches!(pending.capture_mode, HeadingCaptureMode::Required) {
                            if pending.implicit_open {
                                pending.implicit_open = false;
                            } else {
                                pending.capture_depth += 1;
                            }
                            return true;
                        }
                    }
                }
                _ => {}
            }

            let mut buffer = String::new();
            let saved = self.state.pending_heading.take();
            self.visit_element(elem.clone(), &mut buffer);
            self.state.pending_heading = saved;
            if let Some(pending) = self.state.pending_heading.as_mut() {
                pending.capture_buffer.push_str(&buffer);
            }
            return true;
        }

        match kind {
            SyntaxKind::TokenWhiteSpace
            | SyntaxKind::TokenLineBreak
            | SyntaxKind::TokenComment
            | SyntaxKind::ItemBlockComment => return true,
            SyntaxKind::TokenLBracket => {
                if let Some(pending) = self.state.pending_heading.as_mut() {
                    pending.capture_mode = HeadingCaptureMode::Optional;
                    pending.capture_depth = 1;
                    pending.capture_buffer.clear();
                    pending.implicit_open = false;
                    return true;
                }
            }
            SyntaxKind::TokenLBrace => {
                if let Some(pending) = self.state.pending_heading.as_mut() {
                    pending.capture_mode = HeadingCaptureMode::Required;
                    pending.capture_depth = 1;
                    pending.capture_buffer.clear();
                    pending.implicit_open = false;
                    return true;
                }
            }
            _ => {}
        }

        if let Some(pending) = self.state.pending_heading.as_ref() {
            if pending.optional.is_some() || pending.required.is_some() {
                self.flush_pending_heading(output);
            } else {
                self.state.pending_heading = None;
            }
        }

        false
    }

    fn flush_pending_heading(&mut self, output: &mut String) {
        let pending = match self.state.pending_heading.take() {
            Some(pending) => pending,
            None => return,
        };

        let title = pending
            .required
            .or(pending.optional)
            .unwrap_or_default()
            .trim()
            .to_string();

        if title.is_empty() {
            return;
        }

        output.push('\n');
        for _ in 0..=pending.level {
            output.push('=');
        }
        output.push(' ');
        output.push_str(&title);
        output.push('\n');
    }

    /// Convert a complete LaTeX document to Typst with full diagnostics
    ///
    /// Returns both the converted output and any warnings generated during conversion.
    pub fn convert_document_with_diagnostics(&mut self, input: &str) -> ConversionResult {
        let output = self.convert_document(input);
        let warnings = self.state.take_structured_warnings();
        ConversionResult::with_warnings(output, warnings)
    }

    /// Convert math-only LaTeX to Typst with full diagnostics
    ///
    /// Returns both the converted output and any warnings generated during conversion.
    pub fn convert_math_with_diagnostics(&mut self, input: &str) -> ConversionResult {
        let output = self.convert_math(input);
        let warnings = self.state.take_structured_warnings();
        ConversionResult::with_warnings(output, warnings)
    }
}

fn extract_macro_arg(raw: &str, name: &str) -> Option<String> {
    let needle = format!("\\{}", name);
    let mut idx = 0usize;
    while let Some(pos) = raw[idx..].find(&needle) {
        let mut i = idx + pos + needle.len();
        let bytes = raw.as_bytes();
        while i < raw.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= raw.len() || bytes[i] != b'{' {
            idx = i;
            continue;
        }
        let start = i;
        let mut depth = 0i32;
        let mut end = None;
        for (off, ch) in raw[start..].char_indices() {
            if ch == '{' {
                depth += 1;
                continue;
            }
            if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + off);
                    break;
                }
            }
        }
        if let Some(end_idx) = end {
            let content = raw[start + 1..end_idx].trim();
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
        idx = start + 1;
    }
    None
}

fn extract_def_macro(raw: &str, name: &str) -> Option<String> {
    let needle = format!("\\def\\{}", name);
    let mut idx = 0usize;
    while let Some(pos) = raw[idx..].find(&needle) {
        let mut i = idx + pos + needle.len();
        let bytes = raw.as_bytes();
        while i < raw.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= raw.len() || bytes[i] != b'{' {
            idx = i;
            continue;
        }
        let start = i;
        let mut depth = 0i32;
        let mut end = None;
        for (off, ch) in raw[start..].char_indices() {
            if ch == '{' {
                depth += 1;
                continue;
            }
            if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + off);
                    break;
                }
            }
        }
        if let Some(end_idx) = end {
            let content = raw[start + 1..end_idx].trim();
            if !content.is_empty() {
                return Some(convert_caption_text(content));
            }
        }
        idx = start + 1;
    }
    None
}

fn detect_template_from_packages(input: &str) -> Option<(TemplateKind, String)> {
    let packages = extract_usepackage_names(input);
    for pkg in packages {
        let name = pkg.to_lowercase();
        if name.starts_with("cvpr") {
            return Some((TemplateKind::Cvpr, pkg));
        }
        if name.starts_with("aaai") {
            return Some((TemplateKind::Aaai, pkg));
        }
        if name.starts_with("llncs") {
            return Some((TemplateKind::Lncs, pkg));
        }
        if name.starts_with("iclr") {
            return Some((TemplateKind::Iclr, pkg));
        }
        if name.starts_with("icml") {
            return Some((TemplateKind::Icml, pkg));
        }
        if name.starts_with("neurips") {
            return Some((TemplateKind::Neurips, pkg));
        }
        if name.starts_with("jmlr") {
            return Some((TemplateKind::Jmlr, pkg));
        }
        if name.starts_with("tmlr") {
            return Some((TemplateKind::Tmlr, pkg));
        }
        if name.starts_with("rlj") || name.starts_with("rlc") {
            return Some((TemplateKind::Rlj, pkg));
        }
        if name.starts_with("elsarticle") {
            return Some((TemplateKind::Elsevier, pkg));
        }
        if name.starts_with("svjour") || name.starts_with("svproc") {
            return Some((TemplateKind::Springer, pkg));
        }
        if name.starts_with("suthesis") {
            return Some((TemplateKind::StanfordThesis, pkg));
        }
    }
    None
}

fn extract_usepackage_names(input: &str) -> Vec<String> {
    super::utils::collect_usepackage_entries(input)
}

impl Default for LatexConverter {
    fn default() -> Self {
        Self::new()
    }
}

fn dedupe_structured_warnings(warnings: Vec<ConversionWarning>) -> Vec<ConversionWarning> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for warning in warnings {
        let key = format!(
            "{}|{}|{}",
            warning.kind,
            warning.message,
            warning.location.as_deref().unwrap_or("")
        );
        if seen.insert(key) {
            out.push(warning);
        }
    }
    out
}

fn dedupe_string_warnings(warnings: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for warning in warnings {
        if seen.insert(warning.clone()) {
            out.push(warning);
        }
    }
    out
}

fn is_benign_let_target(target: &str) -> bool {
    if target.contains('@') {
        return true;
    }
    matches!(target, "relax" | "mbox" | "allowbreak" | "makeindex")
}

fn is_benign_unsupported_primitive(name: &str) -> bool {
    matches!(
        name,
        "catcode"
            | "lccode"
            | "uccode"
            | "sfcode"
            | "mathcode"
            | "setbox"
            | "box"
            | "copy"
            | "unhbox"
            | "unvbox"
            | "advance"
            | "multiply"
            | "divide"
            | "the"
    )
}

fn capture_geometry_hints(state: &mut ConversionState, input: &str) {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\usepackage") {
        let start = pos + idx + "\\usepackage".len();
        let (opt, after_opt) = extract_bracket_arg_at(input, start);
        let (pkgs, next) = if let Some(pos) = after_opt {
            extract_braced_arg_at(input, pos)
        } else {
            (None, None)
        };
        if let Some(pkgs) = pkgs {
            if pkgs.split(',').any(|p| p.trim() == "geometry") {
                if let Some(opts) = opt.as_deref() {
                    apply_geometry_options_state(state, opts);
                }
            }
        }
        pos = next.unwrap_or(start + 1);
    }

    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\geometry") {
        let start = pos + idx + "\\geometry".len();
        let (arg, next) = extract_braced_arg_at(input, start);
        if let Some(opts) = arg {
            apply_geometry_options_state(state, &opts);
        }
        pos = next.unwrap_or(start + 1);
    }
}

fn capture_length_hints(state: &mut ConversionState, input: &str) {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\setlength") {
        let start = pos + idx + "\\setlength".len();
        let (arg1, next) = extract_braced_arg_at(input, start);
        let (arg2, next2) = if let Some(next) = next {
            extract_braced_arg_at(input, next)
        } else {
            (None, None)
        };
        if let (Some(target), Some(value)) = (arg1, arg2) {
            apply_length_setting_state(state, &target, &value);
        }
        pos = next2.unwrap_or(start + 1);
    }
}

pub(crate) fn capture_fancyhdr_hints(state: &mut ConversionState, input: &str) {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\pagestyle") {
        let start = pos + idx + "\\pagestyle".len();
        let (arg, next) = extract_braced_arg_at(input, start);
        if let Some(style) = arg {
            if style.trim() == "fancy" {
                state.header.enabled = true;
            }
        }
        pos = next.unwrap_or(start + 1);
    }

    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\fancyhead") {
        let mut cursor = pos + idx + "\\fancyhead".len();
        let bytes = input.as_bytes();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let mut opt = String::new();
        if cursor < bytes.len() && bytes[cursor] == b'[' {
            if let (Some(content), Some(next)) = extract_bracket_arg_at(input, cursor) {
                opt = content;
                cursor = next;
            }
        }
        let (arg, next) = extract_braced_arg_at(input, cursor);
        if let Some(content) = arg {
            apply_fancy_head_state(state, opt.as_str(), &content);
        }
        pos = next.unwrap_or(cursor + 1);
    }

    if let Some(idx) = input.find("\\fancyhead[L]") {
        let start = idx + "\\fancyhead[L]".len();
        let (arg, _) = extract_braced_arg_at(input, start);
        if let Some(content) = arg {
            apply_fancy_head_state(state, "L", &content);
        }
    }
    if let Some(idx) = input.find("\\fancyhead[C]") {
        let start = idx + "\\fancyhead[C]".len();
        let (arg, _) = extract_braced_arg_at(input, start);
        if let Some(content) = arg {
            apply_fancy_head_state(state, "C", &content);
        }
    }
    if let Some(idx) = input.find("\\fancyhead[R]") {
        let start = idx + "\\fancyhead[R]".len();
        let (arg, _) = extract_braced_arg_at(input, start);
        if let Some(content) = arg {
            apply_fancy_head_state(state, "R", &content);
        }
    }
}

fn capture_titleformat_hints(state: &mut ConversionState, input: &str) {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\titleformat") {
        let start = pos + idx + "\\titleformat".len();
        let (arg1, next1) = extract_braced_arg_at(input, start);
        let (arg2, next2) = if let Some(next) = next1 {
            extract_braced_arg_at(input, next)
        } else {
            (None, None)
        };
        if let (Some(target), Some(format)) = (arg1, arg2) {
            let level = match target.trim().trim_start_matches('\\') {
                "section" => Some(1),
                "subsection" => Some(2),
                "subsubsection" => Some(3),
                "paragraph" => Some(4),
                "subparagraph" => Some(5),
                _ => None,
            };
            if let Some(level) = level {
                let style = parse_heading_style_from_format(&format);
                state.heading_styles.insert(level, style);
            }
        }
        pos = next2.unwrap_or(start + 1);
    }
}

fn capture_pagenumbering_hints(state: &mut ConversionState, input: &str) {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\pagenumbering") {
        let start = pos + idx + "\\pagenumbering".len();
        let (arg, next) = extract_braced_arg_at(input, start);
        if let Some(style) = arg {
            state.page_numbering = map_pagenumbering_style(&style);
        }
        pos = next.unwrap_or(start + 1);
    }
}

fn extract_braced_arg_at(input: &str, start: usize) -> (Option<String>, Option<usize>) {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return (None, None);
    }
    if bytes[i] != b'{' {
        if let Some(pos) = input[i..].find('{') {
            i += pos;
        } else {
            return (None, None);
        }
    }
    let mut depth = 0i32;
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let content = input[i + 1..j].to_string();
                    return (Some(content), Some(j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    (None, None)
}

fn extract_bracket_arg_at(input: &str, start: usize) -> (Option<String>, Option<usize>) {
    let bytes = input.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return (None, None);
    }
    if bytes[i] != b'[' {
        return (None, Some(i));
    }
    let mut depth = 0i32;
    let mut j = i;
    while j < bytes.len() {
        match bytes[j] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    let content = input[i + 1..j].to_string();
                    return (Some(content), Some(j + 1));
                }
            }
            _ => {}
        }
        j += 1;
    }
    (None, None)
}

fn extract_tylax_hint_block(input: &str) -> String {
    let start_marker = "%__TYLAX_HINTS_BEGIN__";
    let end_marker = "%__TYLAX_HINTS_END__";
    let Some(start_idx) = input.find(start_marker) else {
        return String::new();
    };
    let after_start = &input[start_idx + start_marker.len()..];
    let Some(end_idx) = after_start.find(end_marker) else {
        return String::new();
    };
    let block = &after_start[..end_idx];
    let mut out = String::new();
    for line in block.lines() {
        let trimmed = line.trim_start();
        let decommented = if let Some(rest) = trimmed.strip_prefix('%') {
            rest.strip_prefix(' ').unwrap_or(rest)
        } else {
            line
        };
        out.push_str(decommented);
        out.push('\n');
    }
    out
}

fn strip_abstract_label(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("abstract") {
        return trimmed.to_string();
    }
    let mut rest = trimmed["abstract".len()..].trim_start();
    if let Some(stripped) = rest.strip_prefix(':') {
        rest = stripped.trim_start();
    } else if let Some(stripped) = rest.strip_prefix('-') {
        rest = stripped.trim_start();
    } else if let Some(stripped) = rest.strip_prefix('—') {
        rest = stripped.trim_start();
    }
    if rest.is_empty() {
        trimmed.to_string()
    } else {
        rest.to_string()
    }
}

fn apply_geometry_options_state(state: &mut ConversionState, options: &str) {
    for raw in options.split(',') {
        let opt = raw.trim();
        if opt.is_empty() {
            continue;
        }
        if let Some((key, value)) = opt.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "margin" => state.page_margin.all = Some(value.to_string()),
                "left" => state.page_margin.left = Some(value.to_string()),
                "right" => state.page_margin.right = Some(value.to_string()),
                "top" => state.page_margin.top = Some(value.to_string()),
                "bottom" => state.page_margin.bottom = Some(value.to_string()),
                "hmargin" => {
                    state.page_margin.left = Some(value.to_string());
                    state.page_margin.right = Some(value.to_string());
                }
                "vmargin" => {
                    state.page_margin.top = Some(value.to_string());
                    state.page_margin.bottom = Some(value.to_string());
                }
                "paper" => {
                    state.page_paper = Some(value.to_string());
                }
                _ => {}
            }
            continue;
        }
        if opt.ends_with("paper") && opt.len() > "paper".len() {
            let paper = opt.trim_end_matches("paper");
            if !paper.is_empty() {
                state.page_paper = Some(paper.to_string());
            }
        }
    }
}

/// Normalize LaTeX paper sizes to Typst-compatible names
fn normalize_paper_size(paper: &str) -> &str {
    match paper.to_lowercase().as_str() {
        "letter" | "letterpaper" => "us-letter",
        "legal" | "legalpaper" => "us-legal",
        "executive" | "executivepaper" => "us-executive",
        "tabloid" => "us-tabloid",
        "ledger" => "us-ledger",
        _ => paper, // a4, a5, etc. are already valid in Typst
    }
}

fn apply_length_setting_state(state: &mut ConversionState, target: &str, value: &str) {
    let mut name = target.trim().trim_start_matches('\\').to_string();
    name.retain(|c| c.is_ascii_alphabetic());
    // Handle LaTeX internal dimension macros first
    let raw = value
        .trim()
        .trim_matches(|c| c == '{' || c == '}')
        .replace("\\p@", "pt")
        .replace("\\z@", "0pt")
        .replace("\\@plus", " ")
        .replace("\\@minus", " ");
    let raw = raw.trim();
    let val = if let Some(mult) = raw.strip_suffix("\\baselineskip") {
        let factor = mult.trim();
        if factor.is_empty() {
            "1em".to_string()
        } else if let Ok(value) = factor.parse::<f32>() {
            let mut out = format!("{value:.3}");
            while out.contains('.') && out.ends_with('0') {
                out.pop();
            }
            if out.ends_with('.') {
                out.pop();
            }
            format!("{out}em")
        } else {
            "1em".to_string()
        }
    } else if let Some(rest) = raw.strip_prefix("\\stretch") {
        let rest = rest.trim();
        if let Some(arg) = rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            let inner = arg.trim();
            if !inner.is_empty() {
                format!("{inner}fr")
            } else {
                "1fr".to_string()
            }
        } else {
            "1fr".to_string()
        }
    } else if raw == "\\fill" || raw == "\\hfill" || raw == "\\vfill" {
        "1fr".to_string()
    } else if raw.ends_with("pc") {
        let number = raw.trim_end_matches("pc").trim();
        if let Ok(value) = number.parse::<f32>() {
            let pts = value * 12.0;
            let mut out = format!("{:.4}", pts);
            while out.contains('.') && out.ends_with('0') {
                out.pop();
            }
            if out.ends_with('.') {
                out.pop();
            }
            format!("{out}pt")
        } else {
            raw.to_string()
        }
    } else if raw.parse::<f32>().is_ok() {
        format!("{raw}pt")
    } else {
        // Handle ex unit - Typst parser can misinterpret "0.05ex" as "0.05e" (scientific notation)
        // Convert to multiplication form to avoid parsing issues
        fix_ex_dimension(&raw)
    };
    if name.contains("parskip") {
        state.par_skip = Some(val);
    } else if name.contains("parindent") {
        state.par_indent = Some(val);
    }
}

/// Fix dimension values with 'ex' unit to avoid Typst parser issues.
/// Typst interprets "1ex" as "1e" (scientific notation start) + "x".
/// Convert 'ex' to 'em' with approximate ratio (1ex ≈ 0.43em).
fn fix_ex_dimension(dim: &str) -> String {
    let dim = dim.trim();
    if dim.ends_with("ex") {
        let num_part = dim.trim_end_matches("ex").trim();
        if let Ok(val) = num_part.parse::<f64>() {
            // Convert ex to em (1ex ≈ 0.43em based on typical x-height ratio)
            let em_val = val * 0.43;
            // Format with reasonable precision
            let formatted = format!("{:.4}", em_val);
            let formatted = formatted.trim_end_matches('0').trim_end_matches('.');
            return format!("{}em", formatted);
        }
    }
    dim.to_string()
}

fn apply_fancy_head_state(state: &mut ConversionState, opt: &str, content: &str) {
    let text = super::utils::convert_caption_text(content);
    if opt.trim().is_empty() {
        state.header.left = None;
        state.header.center = None;
        state.header.right = None;
        return;
    }
    let key = opt.trim().to_uppercase();
    if key.contains('L') {
        state.header.left = Some(text.trim().to_string());
    }
    if key.contains('C') {
        state.header.center = Some(text.trim().to_string());
    }
    if key.contains('R') {
        state.header.right = Some(text.trim().to_string());
    }
}

fn parse_heading_style_from_format(format: &str) -> HeadingStyleDef {
    let mut style = HeadingStyleDef::default();
    let fmt = format.replace('{', " ").replace('}', " ");
    let sizes = [
        ("\\Huge", "2em"),
        ("\\huge", "1.8em"),
        ("\\LARGE", "1.6em"),
        ("\\Large", "1.4em"),
        ("\\large", "1.2em"),
        ("\\normalsize", "1em"),
        ("\\small", "0.9em"),
        ("\\footnotesize", "0.8em"),
        ("\\scriptsize", "0.7em"),
        ("\\tiny", "0.6em"),
    ];
    for (latex, size) in sizes {
        if fmt.contains(latex) {
            style.size = Some(size.to_string());
            break;
        }
    }
    if fmt.contains("\\bfseries") || fmt.contains("\\textbf") || fmt.contains("\\bf") {
        style.bold = true;
    }
    if fmt.contains("\\itshape")
        || fmt.contains("\\textit")
        || fmt.contains("\\emph")
        || fmt.contains("\\it")
    {
        style.italic = true;
    }
    style
}

fn map_pagenumbering_style(style: &str) -> Option<String> {
    match style.trim() {
        "arabic" => Some("1".to_string()),
        "roman" => Some("i".to_string()),
        "Roman" => Some("I".to_string()),
        "alph" => Some("a".to_string()),
        "Alph" => Some("A".to_string()),
        "gobble" => None,
        _ => Some("1".to_string()),
    }
}

fn capture_hypersetup_hints(state: &mut ConversionState, input: &str) {
    let mut pos = 0usize;
    while let Some(idx) = input[pos..].find("\\hypersetup") {
        let start = pos + idx + "\\hypersetup".len();
        let after = &input[start..];
        let brace_pos = after.find('{');
        let Some(brace_rel) = brace_pos else {
            pos = start;
            continue;
        };
        let mut depth = 0i32;
        let mut content = String::new();
        let mut started = false;
        for ch in after[brace_rel..].chars() {
            if ch == '{' {
                depth += 1;
                if depth == 1 {
                    started = true;
                    continue;
                }
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            if started {
                content.push(ch);
            }
        }

        for part in content.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim().to_lowercase();
                let value = value.trim();
                if key == "urlcolor" || key == "linkcolor" {
                    if !value.is_empty() {
                        state.link_color = Some(sanitize_color_expression(value));
                    }
                }
            }
        }

        pos = start + brace_rel + content.len();
    }
}

fn capture_color_defs(state: &mut ConversionState, input: &str) {
    let stripped = strip_latex_comments(input);
    let mut pos = 0usize;
    while let Some(idx) = stripped[pos..].find("\\definecolor") {
        let start = pos + idx + "\\definecolor".len();
        let (name, next1) = extract_braced_arg_at(&stripped, start);
        let (model, next2) = if let Some(next) = next1 {
            extract_braced_arg_at(&stripped, next)
        } else {
            (None, None)
        };
        let (spec, next3) = if let Some(next) = next2 {
            extract_braced_arg_at(&stripped, next)
        } else {
            (None, None)
        };
        if let (Some(name), Some(model), Some(spec)) = (name, model, spec) {
            let ident = sanitize_color_identifier(name.trim());
            if !ident.is_empty() {
                let value = parse_color_with_model(model.trim(), spec.trim());
                state.register_color_def(ident, value);
            }
        }
        pos = next3.unwrap_or(start + 1);
    }

    let mut pos = 0usize;
    while let Some(idx) = stripped[pos..].find("\\colorlet") {
        let start = pos + idx + "\\colorlet".len();
        let (name, next1) = extract_braced_arg_at(&stripped, start);
        let (spec, next2) = if let Some(next) = next1 {
            extract_braced_arg_at(&stripped, next)
        } else {
            (None, None)
        };
        if let (Some(name), Some(spec)) = (name, spec) {
            let ident = sanitize_color_identifier(name.trim());
            if !ident.is_empty() {
                let value = sanitize_color_expression(spec.trim());
                state.register_color_def(ident, value);
            }
        }
        pos = next2.unwrap_or(start + 1);
    }
}
