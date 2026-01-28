//! Markup and command handling for LaTeX to Typst conversion
//!
//! This module handles LaTeX commands like \section, \textbf, \cite, etc.

use mitex_parser::syntax::{CmdItem, SyntaxElement};
use rowan::ast::AstNode;
use std::fmt::Write;
use std::time::Instant;

use crate::data::colors::{
    is_named_color, parse_color_with_model, sanitize_color_expression, sanitize_color_identifier,
};
use crate::data::constants::{CodeBlockOptions, LANGUAGE_MAP};
use crate::data::extended_symbols::EXTENDED_SYMBOLS;
use crate::data::maps::TEX_COMMAND_SPEC;
use crate::data::shorthands::apply_shorthand;
use crate::data::symbols::{
    BIBLATEX_COMMANDS, CHAR_COMMANDS, GREEK_LETTERS, LETTER_COMMANDS, MISC_SYMBOLS, NAME_COMMANDS,
    TEXT_FORMAT_COMMANDS,
};
use mitex_spec::CommandSpecItem;

use super::context::{
    CitationMode, ConversionMode, EnvironmentContext, LatexConverter, MacroDef, PendingHeading,
    PendingOperator,
};
use super::utils::{
    convert_caption_text, escape_typst_string, escape_typst_text, sanitize_citation_key,
    sanitize_label, to_roman_numeral,
};
use crate::features::images::{render_image_expr, ImageAttributes};
use crate::utils::loss::{LossKind, LOSS_MARKER_PREFIX};

struct ProfileGuard<'a> {
    label: &'a str,
    start: Instant,
}

impl<'a> ProfileGuard<'a> {
    fn new(label: &'a str) -> Self {
        ProfileGuard {
            label,
            start: Instant::now(),
        }
    }
}

impl<'a> Drop for ProfileGuard<'a> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_secs_f64();
        if elapsed >= 0.05 {
            eprintln!("[tylax] cmd {} total {:.3}s", self.label, elapsed);
        }
    }
}

/// Convert a command symbol (e.g., \alpha, \beta, or special chars like \$, \%)
pub fn convert_command_sym(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    if let SyntaxElement::Token(t) = elem {
        let text = t.text();

        // Skip \begin and \end - these are handled by environment conversion
        if text == "\\begin" || text == "\\end" {
            return;
        }

        // Get the character(s) after backslash
        let cmd_name = &text[1..];

        if cmd_name.is_empty() {
            return;
        }

        // Skip in preamble for most symbols (but not escape chars)
        let is_escape_char = matches!(
            cmd_name,
            "$" | "%" | "&" | "#" | "_" | "{" | "}" | "~" | "@" | "*"
        );
        if conv.state.in_preamble && !is_escape_char {
            return;
        }

        // Handle special character escapes that need proper handling for Typst
        match cmd_name {
            // Characters that need escaping in Typst
            "$" => {
                output.push_str("\\$"); // $ starts math mode in Typst
                return;
            }
            "#" => {
                output.push_str("\\#"); // # starts code mode in Typst
                return;
            }
            "_" => {
                if matches!(conv.state.mode, ConversionMode::Math) {
                    output.push('_');
                } else {
                    output.push_str("\\_"); // _ causes emphasis in text
                }
                return;
            }
            "*" => {
                if matches!(conv.state.mode, ConversionMode::Math) {
                    output.push('*');
                } else {
                    output.push_str("\\*"); // * causes emphasis in text
                }
                return;
            }
            "@" => {
                // \@ in LaTeX is a spacing control that has no Typst equivalent
                // Just strip it - don't output \@ which is invalid in Typst
                return;
            }
            // Characters safe to output directly
            "%" => {
                output.push('%');
                return;
            }
            "&" => {
                output.push('&');
                return;
            }
            "{" => {
                output.push('{');
                return;
            }
            "}" => {
                output.push('}');
                return;
            }
            "~" => {
                output.push('~');
                return;
            }
            "<" => {
                if matches!(conv.state.mode, ConversionMode::Math) {
                    output.push_str("angle.l ");
                } else {
                    output.push('<');
                }
                return;
            }
            ">" => {
                if matches!(conv.state.mode, ConversionMode::Math) {
                    output.push_str("angle.r ");
                } else {
                    output.push('>');
                }
                return;
            }
            _ => {}
        }

        // Handle special options
        if cmd_name == "infty" && conv.state.options.infty_to_oo {
            output.push_str("oo");
            output.push(' ');
            return;
        }

        // Try symbol maps
        if let Some(typst) = lookup_symbol(cmd_name) {
            // In math mode, ensure space before alphabetic symbols if previous char was digit
            // This prevents "3.5times" from being parsed as a single identifier
            if matches!(conv.state.mode, ConversionMode::Math)
                && typst.starts_with(|c: char| c.is_ascii_alphabetic())
                && !output.is_empty()
                && !output.ends_with(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == '{')
            {
                output.push(' ');
            }
            output.push_str(typst);
            output.push(' ');
        } else {
            // In math mode, ensure space before alphabetic command names if previous char was digit
            if matches!(conv.state.mode, ConversionMode::Math)
                && cmd_name.starts_with(|c: char| c.is_ascii_alphabetic())
                && !output.is_empty()
                && !output.ends_with(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == '{')
            {
                output.push(' ');
            }
            // Pass through unknown symbols
            output.push_str(cmd_name);
            output.push(' ');
        }
    }
}

/// Look up a symbol in various symbol tables
fn lookup_symbol(name: &str) -> Option<&'static str> {
    // First check TEX_COMMAND_SPEC for aliases - these give proper Typst symbol names
    if let Some(CommandSpecItem::Cmd(shape)) = TEX_COMMAND_SPEC.get(name) {
        if let Some(ref alias) = shape.alias {
            // Return static string - we leak a bit here but it's acceptable
            return Some(Box::leak(alias.clone().into_boxed_str()));
        }
    }

    // Check extended symbols
    if let Some(typst) = EXTENDED_SYMBOLS.get(name) {
        return Some(*typst);
    }

    let key = format!("\\{}", name);

    // Check misc symbols
    if let Some(typst) = MISC_SYMBOLS.get(key.as_str()) {
        return Some(*typst);
    }

    // Check char commands (e.g., \textquoteleft)
    if let Some(typst) = CHAR_COMMANDS.get(key.as_str()) {
        return Some(*typst);
    }

    // Check Greek letters
    if let Some(typst) = GREEK_LETTERS.get(key.as_str()) {
        return Some(*typst);
    }

    // Check letter commands (e.g., \i, \j)
    if let Some(typst) = LETTER_COMMANDS.get(key.as_str()) {
        return Some(*typst);
    }

    // Check biblatex commands
    if let Some(typst) = BIBLATEX_COMMANDS.get(key.as_str()) {
        return Some(*typst);
    }

    // Check name commands (e.g., \LaTeX, \TeX)
    if let Some(typst) = NAME_COMMANDS.get(key.as_str()) {
        return Some(*typst);
    }

    None
}

fn write_math_class(output: &mut String, class_name: &str, arg: &str) {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return;
    }
    if trimmed.contains('#') {
        let escaped = escape_typst_string(trimmed);
        let _ = write!(output, "class(\"{}\", text(\"{}\")) ", class_name, escaped);
    } else {
        let _ = write!(output, "class(\"{}\", {}) ", class_name, trimmed);
    }
}

fn write_math_text(conv: &mut LatexConverter, cmd: &CmdItem, output: &mut String) {
    let raw = conv
        .get_required_arg_with_braces(cmd, 0)
        .unwrap_or_default();

    // Check if content contains embedded math ($...$)
    if raw.contains('$') {
        write_math_text_with_embedded_math(conv, &raw, output);
    } else {
        let text = convert_caption_text(&raw);
        let escaped = escape_typst_string(text.trim());
        let _ = write!(output, "\"{}\"", escaped);
    }
}

/// Handle \text{} content that contains embedded math like $x$ or $y$
/// Splits into text and math segments and outputs them properly for Typst math mode
fn write_math_text_with_embedded_math(
    conv: &mut LatexConverter,
    raw: &str,
    output: &mut String,
) {
    let mut segments: Vec<String> = Vec::new();
    let mut current_text = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            // Flush accumulated text
            if !current_text.is_empty() {
                let text = convert_caption_text(&current_text);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let escaped = escape_typst_string(trimmed);
                    segments.push(format!("\"{}\"", escaped));
                }
                current_text.clear();
            }

            // Collect math content until closing $
            let mut math_content = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch == '$' {
                    chars.next(); // consume closing $
                    break;
                }
                math_content.push(chars.next().unwrap());
            }

            // Convert the math content
            if !math_content.is_empty() {
                // Simple variable names can be output directly
                // More complex math would need full conversion
                let math_trimmed = math_content.trim();
                if math_trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    // Simple identifier - output as-is
                    segments.push(math_trimmed.to_string());
                } else {
                    // Complex math - try to convert it
                    let converted = conv.convert_inline_math(&math_content);
                    // Strip the surrounding $ if present
                    let cleaned = converted.trim().trim_matches('$').trim();
                    if !cleaned.is_empty() {
                        segments.push(cleaned.to_string());
                    }
                }
            }
        } else {
            current_text.push(ch);
        }
    }

    // Flush any remaining text
    if !current_text.is_empty() {
        let text = convert_caption_text(&current_text);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let escaped = escape_typst_string(trimmed);
            segments.push(format!("\"{}\"", escaped));
        }
    }

    // Join segments with spaces (Typst math juxtaposition)
    if segments.is_empty() {
        output.push_str("\"\"");
    } else if segments.len() == 1 {
        output.push_str(&segments[0]);
    } else {
        // Wrap multiple segments in parentheses for clarity
        output.push('(');
        output.push_str(&segments.join(" "));
        output.push(')');
    }
}

fn write_inline_raw(output: &mut String, content: &str, lang: Option<&str>) {
    let escaped = escape_typst_string(content);
    if let Some(lang) = lang {
        if !lang.is_empty() {
            let _ = write!(output, "#raw(lang: \"{}\", \"{}\")", lang, escaped);
            return;
        }
    }
    let _ = write!(output, "#raw(\"{}\")", escaped);
}

/// Protect content that contains commas by wrapping in `{}`.
///
/// In Typst function calls like `sqrt(content)`, a comma inside `content`
/// would be parsed as an argument separator. Wrapping with `{}` prevents this:
/// - `sqrt(a, b)` → parsed as 2 arguments (error for sqrt)
/// - `sqrt({a, b})` → parsed as 1 argument containing "a, b"
///
/// This function only adds `{}` when necessary (when content contains `,`).
#[inline]
fn protect_comma(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.contains(',') {
        format!("{{{}}}", trimmed)
    } else {
        trimmed.to_string()
    }
}
/// Convert a LaTeX command
pub fn convert_command(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    let node = match &elem {
        SyntaxElement::Node(n) => n.clone(),
        _ => return,
    };

    let cmd = match CmdItem::cast(node.clone()) {
        Some(c) => c,
        None => return,
    };

    let cmd_name = cmd.name_tok().map(|t| t.text().to_string());
    let cmd_str = cmd_name.as_deref().unwrap_or("");

    // Skip empty commands
    if cmd_str.is_empty() {
        return;
    }

    // Remove leading backslash for matching
    let base_name = cmd_str.trim_start_matches('\\');

    if base_name.is_empty() {
        output.push(' ');
        return;
    }

    let _profile_guard = if conv.state.profile_enabled {
        Some(ProfileGuard::new(base_name))
    } else {
        None
    };

    if base_name == "degree"
        && matches!(
            conv.state.template_kind,
            Some(super::context::TemplateKind::Dissertate)
        )
    {
        let raw = conv
            .get_required_arg_with_braces(&cmd, 0)
            .or_else(|| extract_first_braced_arg(&cmd.syntax().text().to_string()))
            .unwrap_or_default();
        if !raw.trim().is_empty() {
            let text = super::utils::convert_caption_text(&raw);
            conv.push_thesis_meta("Degree", text);
        }
        return;
    }

    if base_name == "coloremojicode" {
        let raw = conv
            .get_required_arg_with_braces(&cmd, 0)
            .or_else(|| extract_first_braced_arg(&cmd.syntax().text().to_string()))
            .unwrap_or_default();
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            output.push_str("emoji-");
            output.push_str(trimmed);
        }
        return;
    }

    if base_name == "thispagestyle" || base_name == "pagestyle" {
        let _ = conv.get_required_arg_with_braces(&cmd, 0);
        return;
    }

    // Handle preamble commands
    if conv.state.in_preamble {
        match base_name {
            "documentclass" => {
                if let Some(class) = conv.get_required_arg(&cmd, 0) {
                    conv.state.document_class = Some(class);
                }
                return;
            }
            "usepackage" | "RequirePackage" => {
                if let Some(pkgs) = conv.get_required_arg(&cmd, 0) {
                    if package_list_contains(&pkgs, "geometry") {
                        let opts = conv
                            .get_optional_arg(&cmd, 0)
                            .or_else(|| extract_first_bracket_arg(&cmd.syntax().text().to_string()));
                        if let Some(opts) = opts {
                            apply_geometry_options(conv, &opts);
                        }
                    }
                }
                return;
            }
            "geometry" => {
                if let Some(opts) = conv.get_required_arg(&cmd, 0) {
                    apply_geometry_options(conv, &opts);
                }
                return;
            }
            "setlength" => {
                if let (Some(target), Some(value)) =
                    (conv.get_required_arg(&cmd, 0), conv.get_required_arg(&cmd, 1))
                {
                    apply_length_setting(conv, &target, &value);
                }
                return;
            }
            "parskip" | "parindent" => {
                if let Some(value) = conv.get_required_arg(&cmd, 0) {
                    apply_length_setting(conv, base_name, &value);
                }
                return;
            }
            "onehalfspacing" => {
                conv.state.line_spacing = Some("0.8em".to_string());
                return;
            }
            "doublespacing" => {
                conv.state.line_spacing = Some("1.4em".to_string());
                return;
            }
            "singlespacing" => {
                conv.state.line_spacing = None;
                return;
            }
            "linespread" => {
                if let Some(value) = conv.get_required_arg(&cmd, 0) {
                    apply_line_spread(conv, &value);
                }
                return;
            }
            "setstretch" => {
                if let Some(value) = conv.get_required_arg(&cmd, 0) {
                    apply_line_spread(conv, &value);
                }
                return;
            }
            "pagestyle" => {
                if let Some(style) = conv.get_required_arg(&cmd, 0) {
                    if style.trim() == "fancy" {
                        conv.state.header.enabled = true;
                    }
                }
                return;
            }
            "fancyhead" => {
                apply_fancy_head(conv, &cmd);
                return;
            }
            "titleformat" => {
                apply_titleformat(conv, &cmd);
                return;
            }
            "definecolor" => {
                let name = conv.get_required_arg(&cmd, 0).unwrap_or_default();
                let model = conv.get_required_arg(&cmd, 1).unwrap_or_default();
                let spec = conv.get_required_arg(&cmd, 2).unwrap_or_default();
                if !name.trim().is_empty() && !model.trim().is_empty() && !spec.trim().is_empty() {
                    let ident = sanitize_color_identifier(name.trim());
                    let value = parse_color_with_model(model.trim(), spec.trim());
                    conv.state.register_color_def(ident, value);
                }
                return;
            }
            "colorlet" => {
                let name = conv.get_required_arg(&cmd, 0).unwrap_or_default();
                let spec = conv.get_required_arg(&cmd, 1).unwrap_or_default();
                if !name.trim().is_empty() && !spec.trim().is_empty() {
                    let ident = sanitize_color_identifier(name.trim());
                    let value = sanitize_color_expression(spec.trim());
                    conv.state.register_color_def(ident, value);
                }
                return;
            }
            "title" => {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    let cleaned = super::utils::convert_author_text(&raw);
                    conv.state.title = Some(cleaned.trim().to_string());
                }
                return;
            }
            "author" => {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    if raw.contains("\\IEEEauthorblock") {
                        conv.capture_ieee_author_blocks(&raw);
                    }
                }
                let author = conv.extract_author_arg(&cmd).unwrap_or_default();
                if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                    conv.push_author_block(author);
                } else {
                    conv.state.author = Some(author);
                }
                return;
            }
            "affil" => {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    let text = super::utils::convert_caption_text(&raw);
                    if !text.trim().is_empty() {
                        conv.add_author_line(text);
                    }
                }
                return;
            }
            "date" => {
                conv.state.date = conv.extract_metadata_arg(&cmd);
                return;
            }
            "affiliation" => {
                if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                    if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                        conv.capture_acm_affiliation(&raw);
                    }
                }
                return;
            }
            "institution" | "city" | "country" | "department" => {
                if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                    if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                        let text = super::utils::convert_caption_text(&raw);
                        conv.add_author_line(text);
                    }
                }
                return;
            }
            "email" => {
                if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                    if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                        let text = super::utils::convert_caption_text(&raw);
                        conv.add_author_email(text);
                    }
                }
                return;
            }
            "newcommand" | "renewcommand" | "providecommand" => {
                handle_newcommand(conv, &cmd);
                return;
            }
            "newtheorem" => {
                if let (Some(env), Some(title)) =
                    (conv.get_required_arg(&cmd, 0), conv.get_required_arg(&cmd, 1))
                {
                    let display = super::utils::convert_caption_text(&title);
                    conv.state
                        .custom_theorems
                        .insert(env.trim().to_string(), display.trim().to_string());
                }
                return;
            }
            "def" => {
                handle_def(conv, &cmd);
                return;
            }
            "DeclareMathOperator" | "DeclareMathOperator*" => {
                handle_declare_math_operator(conv, &cmd, base_name.ends_with('*'));
                return;
            }
            "DeclarePairedDelimiter" => {
                handle_declare_paired_delimiter(conv, &cmd);
                return;
            }
            "newacronym" => {
                handle_newacronym(conv, &cmd);
                return;
            }
            "newglossaryentry" => {
                handle_newglossaryentry(conv, &cmd);
                return;
            }
            // Preamble/setup commands to ignore
            "input" | "include" | "includeonly"
            | "bibliographystyle" | "maketitle"
            | "thispagestyle" | "pagenumbering" | "setcounter" | "addtocounter" 
            | "addtolength" | "theoremstyle" 
            | "allowdisplaybreaks" | "numberwithin"
            | "sisetup" | "NewDocumentCommand"
            | "RenewDocumentCommand" | "ProvideDocumentCommand" | "DeclareDocumentCommand"
            // Layout and spacing
            | "baselinestretch"
            // AtBegin/AtEnd hooks
            | "makeatletter" | "makeatother" | "AtBeginDocument" | "AtEndDocument"
            // Environment definitions
            | "newenvironment" | "renewenvironment"
            // Hyperref
            | "hypersetup"
            // Graphics
            | "graphicspath" | "DeclareGraphicsExtensions"
            // Captions and floats
            | "captionsetup" | "floatsetup" | "titlespacing"
            // Lists
            | "setlist"
            // Glossary and acronyms
            | "makeglossaries" | "printglossaries"
            // Table of contents
            | "tableofcontents" | "listoffigures" | "listoftables"
            // Citations
            | "nocite"
            // TeX primitives and conditionals
            | "newif" | "fi" | "else" | "or" 
            | "begingroup" | "endgroup" | "relax"
            // Keywords and IEEEtran / LNCS specific commands
            | "IEEEkeywords" | "keywords" | "IEEEPARstart" | "IEEEpeerreviewmaketitle"
            | "authorrunning" | "titlerunning" | "institute" | "address" | "subjclass"
            | "ccsdesc" | "received"
            // More preamble commands
            | "DeclareOption" | "ProcessOptions" | "ExecuteOptions"
            | "PackageWarning" | "PackageError" | "ClassWarning" | "ClassError"
            // Font and encoding setup
            | "DeclareRobustCommand" | "newrobustcmd" | "robustify"
            | "DeclareFontFamily" | "DeclareFontShape" | "DeclareSymbolFont"
            | "SetSymbolFont" | "DeclareMathSymbol"
            // Listings and minted setup
            | "lstset" | "lstdefinestyle" | "lstdefinelanguage"
            | "usemintedstyle" | "setminted"
            // Additional formatting commands
            | "protect" | "unexpanded" | "expandafter" | "csname" | "endcsname"
            | "let" | "gdef" | "edef" | "xdef" | "futurelet"
            // Conditional flags (often used in preambles)
            | "iftrue" | "iffalse" | "ifx" | "ifnum" | "ifdim" | "ifcat" | "ifmmode" => {
                return;
            }
            _ => {
                // Ignore all other preamble commands.
                return;
            }
        }
    }

    // Check for user-defined macros
    if let Some(macro_def) = conv.state.macros.get(base_name).cloned() {
        let expanded = expand_user_macro(conv, &cmd, &macro_def);
        output.push_str(&expanded);
        return;
    }

    // Handle document commands
    match base_name {
        "newtheorem" => {
            if let (Some(env), Some(title)) =
                (conv.get_required_arg(&cmd, 0), conv.get_required_arg(&cmd, 1))
            {
                let display = super::utils::convert_caption_text(&title);
                conv.state
                    .custom_theorems
                    .insert(env.trim().to_string(), display.trim().to_string());
            }
            return;
        }
        // Section commands - Part gets special formatting with Roman numerals
        "part" => {
            let title = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0));
            let part_num = conv.state.next_counter("part");
            let roman = to_roman_numeral(part_num as usize);
            output.push_str("\n#v(2em)\n");
            output.push_str("#align(center)[\n");
            let _ = writeln!(output, "  #text(1.2em)[Part {}]", roman);
            let _ = writeln!(output, "  #v(0.5em)");
            if let Some(t) = title {
                let _ = writeln!(output, "  #text(2em, weight: \"bold\")[{}]", t);
            }
            output.push_str("]\n");
            output.push_str("#v(2em)\n\n");
        }
        "extrachapter" => {
            let title = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            if let Some(title) = title {
                let _ = write!(output, "\n#heading(numbering: none)[{}]\n", title);
            }
        }
        "chapter" | "chpt" => {
            let title = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());

            if let Some(title) = title {
                output.push('\n');
                output.push_str("= ");
                output.push_str(&title);
                output.push('\n');
            } else {
                let raw = cmd.syntax().text().to_string();
                let (capture_mode, capture_depth) = if raw.ends_with('[') {
                    (super::context::HeadingCaptureMode::Optional, 1)
                } else if raw.ends_with('{') {
                    (super::context::HeadingCaptureMode::Required, 1)
                } else {
                    (super::context::HeadingCaptureMode::None, 0)
                };
                let implicit_open = matches!(
                    capture_mode,
                    super::context::HeadingCaptureMode::Optional
                        | super::context::HeadingCaptureMode::Required
                );
                conv.state.pending_heading = Some(PendingHeading {
                    level: 0,
                    optional: None,
                    required: None,
                    capture_mode,
                    capture_depth,
                    capture_buffer: String::new(),
                    implicit_open,
                });
            }
        }
        // Sectioning - adjust level based on documentclass
        "section" => {
            // article: section = level 1 (=), report/book: section = level 2 (==)
            let is_article = conv.state.document_class.as_deref() == Some("article")
                || conv
                    .state
                    .document_class_info
                    .as_ref()
                    .map(|info| info.class_name == "article")
                    .unwrap_or(false);
            let base_level = if is_article {
                0
            } else {
                1
            };
            convert_section(conv, &cmd, base_level, output);
        }
        "subsection" => {
            let is_article = conv.state.document_class.as_deref() == Some("article")
                || conv
                    .state
                    .document_class_info
                    .as_ref()
                    .map(|info| info.class_name == "article")
                    .unwrap_or(false);
            let base_level = if is_article {
                1
            } else {
                2
            };
            convert_section(conv, &cmd, base_level, output);
        }
        "subsubsection" => {
            let is_article = conv.state.document_class.as_deref() == Some("article")
                || conv
                    .state
                    .document_class_info
                    .as_ref()
                    .map(|info| info.class_name == "article")
                    .unwrap_or(false);
            let base_level = if is_article {
                2
            } else {
                3
            };
            convert_section(conv, &cmd, base_level, output);
        }
        "paragraph" => {
            let title = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "\n#text(weight: \"bold\")[{}]\n", title);
        }
        "subparagraph" => {
            let title = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "\n#emph[{}]\n", title);
        }

        // Text formatting
        "textbf" | "bf" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
                // Prevent trailing backslash from escaping closing bracket
                if content.ends_with('\\') {
                    let _ = write!(output, "#strong[{} ] ", content);
                } else {
                    let _ = write!(output, "#strong[{}] ", content);
                }
            }
        }
        "caps" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let trimmed = content.trim();
                // Prevent trailing backslash from escaping closing bracket
                if trimmed.ends_with('\\') {
                    let _ = write!(output, "#smallcaps[{} ] ", trimmed);
                } else {
                    let _ = write!(output, "#smallcaps[{}] ", trimmed);
                }
            }
        }
        "textit" | "it" | "emph" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
                // Prevent trailing backslash from escaping closing bracket
                if content.ends_with('\\') {
                    let _ = write!(output, "#emph[{} ] ", content);
                } else {
                    let _ = write!(output, "#emph[{}] ", content);
                }
            }
        }
        "MakeUppercase" | "makeuppercase" | "MakeTextUppercase" | "maketextuppercase" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let _ = write!(output, "upper({}) ", content.trim());
            }
        }
        "MakeLowercase" | "makelowercase" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let _ = write!(output, "lower({}) ", content.trim());
            }
        }
        "texttt" | "tt" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
                let cleaned = super::utils::unescape_latex_monospace(content.trim());
                write_inline_raw(output, cleaned.trim(), None);
            }
        }
        "cramped" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                output.push_str(content.trim());
                output.push(' ');
            }
        }
        "mccorrect" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                output.push_str(content.trim());
                output.push(' ');
            }
        }
        "textlatin" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                output.push_str(content.trim());
                output.push(' ');
            }
        }
        "code" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
                let cleaned = super::utils::unescape_latex_monospace(content.trim());
                write_inline_raw(output, cleaned.trim(), None);
            }
        }
        "pkg" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
                let _ = write!(output, "#emph[{}] ", content);
            }
        }
        "underline" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
                let _ = write!(output, "#underline[{}] ", content);
            }
        }
        "textsc" | "sc" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else {
                let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
                let _ = write!(output, "#smallcaps[{}] ", content);
            }
        }
        "smallcaps" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "#smallcaps[{}] ", content.trim());
            }
        }
        "allcaps" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                write_math_text(conv, &cmd, output);
            } else if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "upper({}) ", content.trim());
            }
        }
        "textsuperscript" => {
            let content = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            let _ = write!(output, "#super[{}]", content.trim());
        }
        "textsubscript" => {
            let content = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            let _ = write!(output, "#sub[{}]", content.trim());
        }
        "ding" => {
            let arg = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let symbol = match arg.trim() {
                "51" => "✓",
                "55" => "✗",
                _ => "",
            };
            if !symbol.is_empty() {
                output.push_str(symbol);
            } else if !arg.trim().is_empty() {
                let _ = write!(output, "[{}]", arg.trim());
            }
        }
        "nicefrac" => {
            let num = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let den = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            if !num.is_empty() && !den.is_empty() {
                let _ = write!(output, "{}/{}", num.trim(), den.trim());
            }
        }
        "lipsum" => {
            let arg = conv.get_optional_arg(&cmd, 0).unwrap_or_default();
            let mut words = 60;
            if !arg.is_empty() {
                let cleaned = arg.replace(['[', ']'], "");
                let nums: Vec<i32> = cleaned
                    .split(|c: char| !c.is_ascii_digit())
                    .filter_map(|s| s.parse::<i32>().ok())
                    .collect();
                if let Some(max) = nums.iter().max() {
                    words = (max * 40).max(40) as usize;
                }
            }
            let _ = write!(output, "#lorem({})", words);
        }
        "doi" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                let _ = write!(output, "#link(\"https://doi.org/{}\")[{}]", text.trim(), text.trim());
            }
        }
        "fnref" => {
            // Footnote reference in author lists (elsarticle)
            return;
        }
        "fntext" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let _ = write!(output, "#footnote[{}]", content.trim());
            }
        }
        "cortext" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    let _ = write!(output, "({})", text.trim());
                }
            }
        }
        "ead" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    output.push_str(text.trim());
                }
            }
        }
        "bstctlcite" => {
            return;
        }
        "tablefootmark" => {
            output.push_str("#footnote[]");
        }
        "marginnote" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                if !content.trim().is_empty() {
                    let _ = write!(output, "#footnote[{}]", content.trim());
                }
            }
        }
        "textcircled" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let _ = write!(output, "({})", content.trim());
            }
        }
        "JPCM" => {
            output.push_str("JPCM");
        }
        "thanklessauthor" => {
            if let Some(author) = conv.state.author.as_ref() {
                let escaped = super::utils::escape_typst_text(author);
                output.push_str(&escaped);
            }
        }
        "thanklesstitle" => {
            if let Some(title) = conv.state.title.as_ref() {
                let escaped = super::utils::escape_typst_text(title);
                output.push_str(&escaped);
            }
        }
        "thanklesspublisher" => {
            // Unknown in most documents; ignore.
        }
        "lhcb" => output.push_str("LHCb"),
        "pythia" => output.push_str("Pythia"),
        "evtgen" => output.push_str("EvtGen"),
        "geant" => output.push_str("Geant"),
        "ce" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                if conv.state.mode == ConversionMode::Math {
                    // mhchem content can include nested LaTeX (e.g. $\beta$) or equation syntax.
                    // Fall back to text when input looks complex so we don't emit invalid Typst math.
                    let is_simple = raw.chars().all(|c| {
                        c.is_ascii_alphanumeric()
                            || c.is_whitespace()
                            || c == '('
                            || c == ')'
                            || c == '.'
                    });
                    if is_simple {
                        let formatted = super::utils::format_chemical_formula_math(&raw);
                        if !formatted.is_empty() {
                            let _ = write!(output, "{} ", formatted);
                        }
                    } else {
                        let text = super::utils::sanitize_ce_text_for_math(&raw);
                        let text = super::utils::strip_unescaped_dollars(&text);
                        let escaped = super::utils::escape_typst_string(text.trim());
                        if !escaped.is_empty() {
                            let _ = write!(output, "text(\"{}\") ", escaped);
                        }
                    }
                } else {
                    let text = super::utils::convert_caption_text(&raw);
                    let escaped = super::utils::escape_typst_string(text.trim());
                    let _ = write!(output, "#text(\"{}\")", escaped);
                }
            }
        }
        "erf" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "op(\"erf\")({})", arg);
            } else {
                output.push_str("op(\"erf\")");
            }
        }
        "erfc" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "op(\"erfc\")({})", arg);
            } else {
                output.push_str("op(\"erfc\")");
            }
        }
        "symbfit" | "symbfup" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&arg);
            }
        }
        "EntryHeading" => {
            if let Some(title) = conv.convert_required_arg(&cmd, 0).or_else(|| conv.get_required_arg(&cmd, 0)) {
                let _ = writeln!(output, "\n#text(weight: \"bold\")[{}]\n", title.trim());
            }
        }
        "entry" => {
            let term = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            let desc = conv
                .convert_required_arg(&cmd, 1)
                .or_else(|| conv.get_required_arg(&cmd, 1))
                .unwrap_or_default();
            let _ = writeln!(
                output,
                "- #strong[{}] — {}",
                term.trim(),
                desc.trim()
            );
        }
        // Text in math - these commands output text in math mode
        "text" | "textrm" | "textup" | "textnormal" => {
            let raw = conv
                .get_required_arg_with_braces(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0));
            if let Some(raw) = raw {
                if conv.state.mode == ConversionMode::Math && raw.contains('$') {
                    // Handle embedded math like \text{judgment $x$ is better than $y$}
                    write_math_text_with_embedded_math(conv, &raw, output);
                    output.push(' ');
                } else {
                    let text = super::utils::convert_caption_text(&raw);
                    let escaped = super::utils::escape_typst_string(text.trim());
                    let _ = write!(output, "\"{}\" ", escaped);
                }
            }
        }

        // Title/author handling that appears inside the document (IEEE templates)
        "title" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let cleaned = raw.replace("\\\\", " ");
                conv.state.title =
                    Some(super::utils::convert_caption_text(&cleaned).trim().to_string());
            }
            return;
        }
        "author" => {
            let author = conv.extract_author_arg(&cmd).unwrap_or_default();
            if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                conv.push_author_block(author);
            } else {
                conv.state.author = Some(author);
            }
            return;
        }
        "affil" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.add_author_line(text);
                }
            }
            return;
        }
        "Author" => {
            let name = conv
                .get_required_arg_with_braces(&cmd, 0)
                .map(|raw| super::utils::convert_author_text(&raw))
                .unwrap_or_default();
            let dept = conv
                .get_required_arg_with_braces(&cmd, 1)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            conv.push_author_block(name);
            if !dept.trim().is_empty() {
                conv.add_author_line(dept);
            }
            for idx in 0..6 {
                if let Some(opt) = conv.get_optional_arg(&cmd, idx) {
                    let text = super::utils::convert_caption_text(&opt);
                    conv.push_thesis_meta("Previous degree", text);
                }
            }
            return;
        }
        "Degree" => {
            let degree = conv
                .get_required_arg_with_braces(&cmd, 0)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let dept = conv
                .get_required_arg_with_braces(&cmd, 1)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let value = if dept.trim().is_empty() {
                degree
            } else {
                format!("{} ({})", degree.trim(), dept.trim())
            };
            conv.push_thesis_meta("Degree", value);
            return;
        }
        "degreeaward" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree award", text);
            }
            return;
        }
        "university" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("University", text);
            }
            return;
        }
        "unilogo" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("University logo", text);
            }
            return;
        }
        "copyyear" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Copyright year", text);
            }
            return;
        }
        "defenddate" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Defense date", text);
            }
            return;
        }
        "rightsstatement" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Rights statement", text);
            }
            return;
        }
        "publishedas" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Published as", text);
            }
            return;
        }
        "pocketmaterial" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Pocket material", text);
            }
            return;
        }
        "Supervisor" => {
            let name = conv
                .get_required_arg_with_braces(&cmd, 0)
                .map(|raw| super::utils::convert_author_text(&raw))
                .unwrap_or_default();
            let title = conv
                .get_required_arg_with_braces(&cmd, 1)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let dept = conv.get_optional_arg(&cmd, 0).unwrap_or_default();
            let dept = super::utils::convert_caption_text(&dept);
            let mut parts = Vec::new();
            if !name.trim().is_empty() {
                parts.push(name.trim().to_string());
            }
            if !title.trim().is_empty() {
                parts.push(title.trim().to_string());
            }
            if !dept.trim().is_empty() {
                parts.push(dept.trim().to_string());
            }
            conv.push_thesis_meta("Supervisor", parts.join(", "));
            return;
        }
        "supervisor" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Supervisor", text);
            }
            return;
        }
        "Acceptor" => {
            let name = conv
                .get_required_arg_with_braces(&cmd, 0)
                .map(|raw| super::utils::convert_author_text(&raw))
                .unwrap_or_default();
            let title = conv
                .get_required_arg_with_braces(&cmd, 1)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let role = conv
                .get_required_arg_with_braces(&cmd, 2)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let mut parts = Vec::new();
            if !name.trim().is_empty() {
                parts.push(name.trim().to_string());
            }
            if !title.trim().is_empty() {
                parts.push(title.trim().to_string());
            }
            if !role.trim().is_empty() {
                parts.push(role.trim().to_string());
            }
            conv.push_thesis_meta("Acceptor", parts.join(", "));
            return;
        }
        "Reader" => {
            let name = conv
                .get_required_arg_with_braces(&cmd, 0)
                .map(|raw| super::utils::convert_author_text(&raw))
                .unwrap_or_default();
            let title = conv
                .get_required_arg_with_braces(&cmd, 1)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let dept = conv
                .get_required_arg_with_braces(&cmd, 2)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let mut parts = Vec::new();
            if !name.trim().is_empty() {
                parts.push(name.trim().to_string());
            }
            if !title.trim().is_empty() {
                parts.push(title.trim().to_string());
            }
            if !dept.trim().is_empty() {
                parts.push(dept.trim().to_string());
            }
            conv.push_thesis_meta("Reader", parts.join(", "));
            return;
        }
        "DegreeDate" => {
            let month = conv
                .get_required_arg_with_braces(&cmd, 0)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let year = conv
                .get_required_arg_with_braces(&cmd, 1)
                .map(|raw| super::utils::convert_caption_text(&raw))
                .unwrap_or_default();
            let value = format!("{} {}", month.trim(), year.trim()).trim().to_string();
            conv.push_thesis_meta("Degree date", value);
            return;
        }
        "ThesisDate" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Thesis date", text);
            }
            return;
        }
        "dept" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Department", text);
            }
            return;
        }
        "principaladviser" | "advisor" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Advisor", text);
            }
            return;
        }
        "coadvisorOne" | "coadvisorTwo" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Co-advisor", text);
            }
            return;
        }
        "firstreader" | "secondreader" | "thirdreader" | "fourthreader" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Reader", text);
            }
            return;
        }
        "committeeInternalOne" | "committeeInternalTwo" | "committeeInternal" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Committee (internal)", text);
            }
            return;
        }
        "committeeExternal" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Committee (external)", text);
            }
            return;
        }
        "chair" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Chair", text);
            }
            return;
        }
        "othermembers" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Committee members", text);
            }
            return;
        }
        "numberofmembers" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Committee size", text);
            }
            return;
        }
        "field" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Field", text);
            }
            return;
        }
        "degreeyear" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree year", text);
            }
            return;
        }
        "degreesemester" | "degreeterm" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree term", text);
            }
            return;
        }
        "degreemonth" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree month", text);
            }
            return;
        }
        "pdOneName" | "pdTwoName" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Previous degree", text);
            }
            return;
        }
        "pdOneSchool" | "pdTwoSchool" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Previous institution", text);
            }
            return;
        }
        "pdOneYear" | "pdTwoYear" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Previous degree year", text);
            }
            return;
        }
        "prefacesection" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                let _ = writeln!(output, "\n= {}\n", text.trim());
            }
            return;
        }
        "qauthor" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                let _ = writeln!(output, "\n#align(right)[— {}]\n", text.trim());
            }
            return;
        }
        "newthought" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                let _ = write!(output, "#smallcaps[{}]", text.trim());
            }
            return;
        }
        "lettrine" => {
            let first = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let rest = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            output.push_str(&format!("{}{}", first, rest));
            return;
        }
        "icmltitle" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let cleaned = raw.replace("\\\\", " ");
                conv.state.title =
                    Some(super::utils::convert_caption_text(&cleaned).trim().to_string());
            }
            return;
        }
        "icmlauthor" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let name = super::utils::convert_author_text(&raw).trim().to_string();
                let keys_raw = conv.get_required_arg_with_braces(&cmd, 1).unwrap_or_default();
                let keys = parse_affiliation_keys(&keys_raw);
                conv.push_author_block_with_affils(name, keys);
            }
            return;
        }
        "icmlaffiliation" => {
            let key = conv.get_required_arg_with_braces(&cmd, 0).unwrap_or_default();
            let value = conv.get_required_arg_with_braces(&cmd, 1).unwrap_or_default();
            let text = super::utils::convert_caption_text(&value);
            conv.add_affiliation_mapping(key, text);
            return;
        }
        "icmlcorrespondingauthor" => {
            let name = conv.get_required_arg_with_braces(&cmd, 0).unwrap_or_default();
            let email = conv.get_required_arg_with_braces(&cmd, 1).unwrap_or_default();
            let name = super::utils::convert_author_text(&name);
            let email = super::utils::convert_caption_text(&email);
            conv.set_author_email_by_name(&name, email);
            return;
        }
        "icmlkeywords" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.state.keywords = text
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            return;
        }
        "twocolumn" => {
            // Process optional content inside \twocolumn[...]
            for child in cmd.syntax().children_with_tokens() {
                if child.kind() == mitex_parser::syntax::SyntaxKind::ClauseArgument {
                    if let SyntaxElement::Node(n) = child {
                        let is_bracket = n
                            .children()
                            .any(|c| c.kind() == mitex_parser::syntax::SyntaxKind::ItemBracket);
                        if is_bracket {
                            for content in n.children_with_tokens() {
                                match content.kind() {
                                    mitex_parser::syntax::SyntaxKind::TokenLBracket
                                    | mitex_parser::syntax::SyntaxKind::TokenRBracket => continue,
                                    _ => conv.visit_element(content, output),
                                }
                            }
                        }
                    }
                }
            }
            return;
        }
        "affiliation" => {
            if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    conv.capture_acm_affiliation(&raw);
                }
                return;
            }
        }
        "institution" | "city" | "country" | "department" => {
            if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    let text = super::utils::convert_caption_text(&raw);
                    conv.add_author_line(text);
                }
                return;
            }
            if base_name == "department" {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    let text = super::utils::convert_caption_text(&raw);
                    conv.push_thesis_meta("Department", text);
                }
                return;
            }
        }
        "email" => {
            if conv.state.template_kind == Some(super::context::TemplateKind::Acm) {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    let text = super::utils::convert_caption_text(&raw);
                    conv.add_author_email(text);
                }
                return;
            }
        }
        "maketitle" | "IEEEpeerreviewmaketitle" => {
            // Title rendering is handled by templates or title blocks
            return;
        }
        "IEEEtitleabstractindextext" => {
            // Capture abstract/keywords stored for IEEE templates.
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                if conv.state.template_kind != Some(super::context::TemplateKind::Ieee) {
                    output.push_str(content.trim());
                }
            }
            return;
        }
        "IEEEdisplaynontitleabstractindextext" => {
            // No-op: content already captured by IEEEtitleabstractindextext
            return;
        }
        "IEEEraisesectionheading" => {
            // Prefer standard section headings; ignore raised heading wrapper.
            return;
        }

        // Labels and references
        "label" => {
            // Skip label output if we're inside equation/align environments
            // because those environments handle labels at the end of the math block
            if conv.state.is_inside(&EnvironmentContext::Equation)
                || conv.state.is_inside(&EnvironmentContext::Align)
                || matches!(conv.state.current_env(), EnvironmentContext::Theorem(_))
            {
                return;
            }
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean_label = sanitize_label(&label);
            if attach_label_to_heading(output, &clean_label) {
                return;
            }
            // Check if output ends with something that can't take a label (e.g., #set)
            // In that case, wrap the label in empty content to make it valid
            let trimmed = output.trim_end();
            if trimmed.ends_with(')') && (trimmed.contains("#set ") || trimmed.contains("#counter(")) {
                let _ = write!(output, "[] <{}>", clean_label);
            } else {
                let _ = write!(output, "<{}>", clean_label);
            }
        }
        "namedlabel" => {
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let text = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let clean_label = sanitize_label(&label);
            if !text.trim().is_empty() {
                output.push_str(text.trim());
            }
            if !clean_label.is_empty() {
                let _ = write!(output, " <{}>", clean_label);
            }
        }
        "IEEEPARstart" => {
            let first = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let rest = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if !first.trim().is_empty() || !rest.trim().is_empty() {
                output.push_str(first.trim());
                output.push_str(rest.trim());
            }
        }
        "appendices" => {
            output.push_str("\n// Appendix\n");
            output.push_str("#counter(heading).update(0)\n");
            output.push_str("#set heading(numbering: \"A.\")\n\n");
        }
        "MR" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(arg.trim());
                output.push(' ');
            }
        }
        "smash" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(content.trim());
            }
        }
        "numberthis" => {
            // Common in align* to force equation numbers; treat as no-op.
        }
        "ref" | "autoref" | "cref" | "Cref" | "cref*" | "Cref*" | "crefrange" | "Crefrange"
        | "secref" | "figref" | "Figref" | "tabref" | "thmref" | "lemref" | "propref"
        | "appref" | "Appref" => {
            let labels = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let items: Vec<String> = labels
                .split(',')
                .map(|s| sanitize_label(s.trim()))
                .filter(|s| !s.is_empty())
                .collect();
            if items.is_empty() {
                return;
            }
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    output.push_str(", ");
                }
                let _ = write!(output, "__TYLAX_REF__{}__", item);
            }
        }
        "eqref" => {
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let sanitized = sanitize_label(&label);
            let _ = write!(output, "__TYLAX_EQREF__{}__", sanitized);
        }
        "pageref" => {
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean_label = sanitize_label(&label);
            let _ = write!(output, "__TYLAX_PAGEREF__{}__", clean_label);
        }
        "cpageref" => {
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean_label = sanitize_label(&label);
            let _ = write!(output, "__TYLAX_PAGEREF__{}__", clean_label);
        }
        "nameref" => {
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean_label = sanitize_label(&label);
            let _ = write!(output, "__TYLAX_REF__{}__", clean_label);
        }

        // Math in text mode
        "ensuremath" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    return;
                }
                if conv.state.mode == ConversionMode::Math {
                    output.push_str(trimmed);
                    output.push(' ');
                } else {
                    let _ = write!(output, "$ {} $", trimmed);
                }
            }
            return;
        }

        // Keywords (IEEE/ACM style)
        "IEEEkeywords" | "keywords" => {
            if conv.state.template_kind == Some(super::context::TemplateKind::Ieee) {
                if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                    let text = super::utils::convert_caption_text(&raw);
                    conv.state.keywords = text
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                return;
            }
        }
        "keyword" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.state.keywords = text
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            return;
        }
        "keywordname" => {
            output.push_str("Keywords");
            output.push(' ');
            return;
        }
        "abstract" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.state.abstract_text = Some(text.trim().to_string());
                }
            }
            return;
        }

        // Bibliography
        "bibliography" | "addbibresource" | "bibdata" => {
            let raw = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let entries: Vec<String> = raw
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if s.ends_with(".bib") {
                        s.to_string()
                    } else {
                        format!("{}.bib", s)
                    }
                })
                .collect();
            if !entries.is_empty() {
                let quoted: Vec<String> = entries
                    .into_iter()
                    .map(|s| format!("\"{}\"", s))
                    .collect();
                if quoted.len() == 1 {
                    let _ = write!(output, "#bibliography({})", quoted.join(", "));
                } else {
                    let _ = write!(output, "#bibliography(({}))", quoted.join(", "));
                }
            }
        }
        "bibinfo" => {
            let _field = conv.get_required_arg(&cmd, 0);
            let value = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if !value.trim().is_empty() {
                output.push_str(value.trim());
            }
        }

        // Prettyref - treat as normal reference
        "prettyref" => {
            let label = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean_label = sanitize_label(&label);
            let _ = write!(output, "@{}", clean_label);
        }

        // Citations - Full set from l2t.rs (40+ variants)
        "cite" | "Cite" | "citep" | "citep*" | "citet" | "citet*" | "citeal"
        | "citealp" | "citealp*" | "citealt" | "citealt*" | "citeA" | "footcitearticle" | "footcitebook"
        | "autocite" | "Autocite" | "textcite" | "Textcite"
        | "parencite" | "Parencite" | "footcite" | "Footcite"
        | "smartcite" | "Smartcite" | "supercite" | "fullcite"
        | "footfullcite" | "cites" | "Cites" | "textcites" | "Textcites"
        | "parencites" | "Parencites" | "autocites" | "Autocites"
        | "shortcite" | "shortciteN" | "shortciteNP" | "citeN" | "citeNP" => {
            let mut keys = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if keys.trim().is_empty() {
                if let Some(fallback) =
                    extract_first_braced_arg(&cmd.syntax().text().to_string())
                {
                    keys = fallback;
                }
            }
            let opt_pre = conv.get_optional_arg(&cmd, 0);
            let opt_post = conv.get_optional_arg(&cmd, 1);
            let mut cleaned_keys: Vec<String> = Vec::new();
            for key in keys.split(',') {
                let clean = sanitize_citation_key(key.trim());
                if !clean.is_empty() {
                    cleaned_keys.push(clean);
                }
            }
            let pre_note = opt_pre
                .map(|note| note.trim().to_string())
                .filter(|note| !note.is_empty());
            let post_note = opt_post
                .map(|note| note.trim().to_string())
                .filter(|note| !note.is_empty());
            if let Some(note) = pre_note {
                let mut escaped = escape_typst_text(&note);
                escaped = escaped.replace('[', "\\[").replace(']', "\\]");
                if !escaped.is_empty() {
                    output.push_str(&escaped);
                    output.push(' ');
                }
            }
            if !cleaned_keys.is_empty() {
                output.push('[');
                for (i, key) in cleaned_keys.iter().enumerate() {
                    if i > 0 {
                        output.push_str("; ");
                    }
                    if matches!(conv.state.citation_mode, CitationMode::Typst) {
                        let _ = write!(output, "@{}", key);
                    } else {
                        output.push_str(&escape_typst_text(key));
                    }
                }
                output.push(']');
            }
            if let Some(note) = post_note {
                let mut escaped = escape_typst_text(&note);
                escaped = escaped.replace('[', "\\[").replace(']', "\\]");
                if !escaped.is_empty() {
                    let _ = write!(output, " [{}]", escaped);
                }
            }
        }
        "citeauthor" | "citeauthor*" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean = sanitize_citation_key(key.trim());
            if clean.is_empty() {
                return;
            }
            if matches!(conv.state.citation_mode, CitationMode::Typst) {
                let _ = write!(output, "#cite(<{}>, form: \"author\")", clean);
            } else {
                output.push_str(&escape_typst_text(&clean));
            }
        }
        "citeyear" | "citeyear*" | "citeyearpar" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean = sanitize_citation_key(key.trim());
            if clean.is_empty() {
                return;
            }
            if matches!(conv.state.citation_mode, CitationMode::Typst) {
                let _ = write!(output, "#cite(<{}>, form: \"year\")", clean);
            } else {
                output.push_str(&escape_typst_text(&clean));
            }
        }
        "yrcite" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean = sanitize_citation_key(key.trim());
            if clean.is_empty() {
                return;
            }
            if matches!(conv.state.citation_mode, CitationMode::Typst) {
                let _ = write!(output, "#cite(<{}>, form: \"year\")", clean);
            } else {
                output.push_str(&escape_typst_text(&clean));
            }
        }
        "citetitle" | "citetitle*" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean = sanitize_citation_key(key.trim());
            if clean.is_empty() {
                return;
            }
            if matches!(conv.state.citation_mode, CitationMode::Typst) {
                let _ = write!(output, "#cite(<{}>, form: \"title\")", clean);
            } else {
                output.push_str(&escape_typst_text(&clean));
            }
        }

        // URLs and hyperlinks
        "url" => {
            if let Some(url) = conv.get_required_arg(&cmd, 0) {
                let _ = write!(output, "#link(\"{}\")", url);
            }
        }
        "hrefurl" => {
            if let Some(url) = conv.get_required_arg(&cmd, 0) {
                let _ = write!(output, "#link(\"{}\")[{}]", url, url);
            }
        }
        "href" => {
            let url = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let text = conv
                .convert_required_arg(&cmd, 1)
                .unwrap_or_else(|| escape_typst_text(&url));
            let _ = write!(output, "#link(\"{}\")[{}]", url, text);
        }
        "link" => {
            let label = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let url = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            if url.trim().is_empty() {
                output.push_str(&label);
            } else if label.trim().is_empty() {
                let _ = write!(output, "#link(\"{}\")[{}]", url, escape_typst_text(&url));
            } else {
                let _ = write!(output, "#link(\"{}\")[{}]", url, label);
            }
        }
        "hyperref" => {
            if let Some(label) = conv.get_optional_arg(&cmd, 0) {
                let text = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
                let clean = sanitize_label(label.trim());
                if clean.is_empty() {
                    output.push_str(&text);
                } else {
                    let _ = write!(output, "#link(<{}>)[{}]", clean, text);
                }
            } else {
                let url = conv.get_required_arg(&cmd, 0).unwrap_or_default();
                let text = conv
                    .convert_required_arg(&cmd, 1)
                    .unwrap_or_else(|| escape_typst_text(&url));
                let _ = write!(output, "#link(\"{}\")[{}]", url, text);
            }
        }

        // Footnotes
        "footnote" | "endnote" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#footnote[{}]", content);
        }
        "footnotetext" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#footnote[{}]", content);
        }
        "footnotemark" => {
            output.push_str("#super[]");
        }
        "tnote" => {
            // Threeparttable table note marker: render as a superscript marker.
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let trimmed = content.trim();
            if trimmed.is_empty() {
                output.push_str("#super[]");
            } else {
                let _ = write!(output, "#super[{}]", trimmed);
            }
        }

        // Conditional logic (best-effort)
        "ifthenelse" => {
            let then_branch = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let else_branch = conv.convert_required_arg(&cmd, 2).unwrap_or_default();
            let trimmed_then = then_branch.trim();
            if trimmed_then.is_empty() {
                output.push_str(else_branch.trim());
            } else {
                output.push_str(trimmed_then);
            }
        }
        "equal" => {
            // Used inside \ifthenelse; ignore here.
        }
        "foreignlanguage" => {
            // \foreignlanguage{lang}{text} -> text
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if !content.trim().is_empty() {
                output.push_str(content.trim());
            }
        }

        // References
        "subref" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let clean = sanitize_label(key.trim());
            let _ = write!(output, "@{}", clean);
        }

        // Graphics - use images module for proper parsing
        "includegraphics" => {
            let options = conv.get_optional_arg(&cmd, 0).unwrap_or_default();
            let path = conv.get_required_arg(&cmd, 0).unwrap_or_default();

            let attrs = ImageAttributes::parse(&options);
            let expr = render_image_expr(&path, &attrs);
            output.push('#');
            output.push_str(&expr);
        }

        // Scalebox
        "scalebox" => {
            let h = conv.get_required_arg(&cmd, 0).unwrap_or_else(|| "1".to_string());
            let v = conv.get_optional_arg(&cmd, 0).unwrap_or_else(|| h.clone());
            if let Some(content) = conv.convert_required_arg(&cmd, 1) {
                let _ = write!(
                    output,
                    "#scale(x: {}, y: {})[{}] ",
                    h.trim(),
                    v.trim(),
                    content
                );
            }
        }

        // Listings
        "lstinputlisting" => {
            let path = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if path.trim().is_empty() {
                output.push_str("#raw(block: true, lang: \"text\", \"\")");
            } else {
                let escaped = super::utils::escape_typst_string(path.trim());
                let _ = write!(
                    output,
                    "#raw(block: true, lang: \"text\", \"Listing: {}\")",
                    escaped
                );
            }
        }

        "subcaptionbox" => {
            let raw_caption = conv.get_required_arg_with_braces(&cmd, 0).unwrap_or_default();
            let (caption, label) = super::utils::strip_label_from_text(&raw_caption);
            let caption_text = super::utils::convert_caption_text(&caption);
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();

            output.push_str("#figure(kind: \"subfigure\", supplement: none");
            if !caption_text.trim().is_empty() {
                let _ = write!(output, ", caption: [{}]", caption_text.trim());
            }
            output.push_str(")[\n");
            if !content.trim().is_empty() {
                output.push_str(content.trim());
                output.push('\n');
            }
            output.push_str("]");
            if let Some(lbl) = label {
                let clean = sanitize_label(&lbl);
                if !clean.is_empty() {
                    let _ = write!(output, " <{}>", clean);
                }
            }
        }

        "centerline" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let _ = write!(output, "#align(center)[{}]", content.trim());
            }
        }

        "rotatebox" => {
            // \rotatebox[origin=c]{90}{content}
            let _opt = conv.get_optional_arg(&cmd, 0);
            let angle = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let mut angle_val = angle.trim().to_string();
            if !angle_val.ends_with("deg") && !angle_val.ends_with("rad") {
                angle_val.push_str("deg");
            }
            if !content.trim().is_empty() {
                let _ = write!(output, "#rotate({})[{}]", angle_val, content.trim());
            }
        }

        "smashoperator" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(arg.trim());
            }
        }

        // Table header helpers (OxEngThesis)
        "tableHeaderStart" | "tableHeaderEnd" => {
            // Style-only commands; ignore.
        }
        "tableHCell" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#strong[{}]", content.trim());
        }
        "tableHCellTwoRows" => {
            let top = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let bottom = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let _ = write!(output, "#strong[{}] #linebreak() {}", top.trim(), bottom.trim());
        }

        // Simple boxes and TODOs
        "tcbox" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#box(stroke: 0.5pt, inset: 2pt)[{}]", content);
        }
        "todo" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#text(fill: red)[{}]", content);
        }
        "doccmd" | "doccmddef" | "doccmdnoindex" => {
            let name = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let cmd_text = format!("\\{}", name.trim());
            let escaped = super::utils::escape_typst_string(&cmd_text);
            let _ = write!(output, "#raw(\"{}\")", escaped);
        }
        "ab" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(content.trim());
        }
        "dotfill" => {
            output.push_str("...");
        }
        "rule" => {
            let width = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let height = conv.get_required_arg(&cmd, 1).unwrap_or_else(|| "1pt".to_string());
            let length = if let Some(val) = width.trim().strip_suffix("\\linewidth") {
                let factor = val.trim();
                if factor.is_empty() {
                    "100%".to_string()
                } else if let Ok(num) = factor.parse::<f64>() {
                    format!("{}%", num * 100.0)
                } else {
                    width.trim().to_string()
                }
            } else if let Some(val) = width.trim().strip_suffix("\\textwidth") {
                let factor = val.trim();
                if factor.is_empty() {
                    "100%".to_string()
                } else if let Ok(num) = factor.parse::<f64>() {
                    format!("{}%", num * 100.0)
                } else {
                    width.trim().to_string()
                }
            } else {
                width.trim().to_string()
            };
            let stroke = height.trim();
            let _ = write!(output, "#line(length: {}, stroke: {})", length, stroke);
        }

        // Page/section helpers
        "clearpage" | "cleardoublepage" | "cleartooddpage" | "myclearpage" | "blankpage" => {
            let is_two_column = conv
                .state
                .document_class_info
                .as_ref()
                .map(|info| info.columns > 1)
                .unwrap_or(false)
                || matches!(
                    conv.state.template_kind,
                    Some(
                        super::context::TemplateKind::Cvpr
                            | super::context::TemplateKind::Iclr
                            | super::context::TemplateKind::Icml
                            | super::context::TemplateKind::Neurips
                    )
                );
            if is_two_column {
                output.push_str("#colbreak()");
            } else {
                output.push_str("#pagebreak()");
            }
        }
        "mypagestyle" | "mylisthead" => {
            // Consume args, ignore styling
        }
        "makefrontmatterpages" | "listofreferences" => {
            // Ignore
        }
        "input" | "include" => {
            // Already expanded during preprocessing; ignore any remaining.
        }
        "onehalfspacing" | "doublespacing" | "singlespacing" => {
            // Ignore spacing commands in body.
        }
        "fill" => {
            output.push_str("#v(1fr)");
        }
        "hrulefill" => {
            output.push_str("#line(length: 100%)");
        }
        "hrule" => {
            output.push_str("#line(length: 100%)");
        }
        "nomname" => {
            output.push_str("Nomenclature");
        }
        "printnomenclature" => {
            // Ignore output; Typst doesn't generate nomenclature here.
        }
        "theauthor" => {
            if let Some(author) = conv.state.author.as_ref() {
                output.push_str(author);
            }
        }
        "@author" => {
            if let Some(author) = conv.state.author.as_ref() {
                output.push_str(author);
            }
        }
        "thetitle" => {
            if let Some(title) = conv.state.title.as_ref() {
                output.push_str(title);
            }
        }
        "@title" => {
            if let Some(title) = conv.state.title.as_ref() {
                output.push_str(title);
            }
        }
        "thedate" => {
            if let Some(date) = conv.state.date.as_ref() {
                output.push_str(date);
            }
        }
        "@date" => {
            if let Some(date) = conv.state.date.as_ref() {
                output.push_str(date);
            }
        }
        "pagenumbering" | "thesistitlepage" | "addcontentsline" => {
            // Ignore pagination and TOC wiring.
        }
        "thepage" => {
            output.push_str("#context counter(page).display()");
        }
        "endfirsthead" | "endfoot" | "endlastfoot" => {
            // longtable control commands; ignore.
        }
        "blindmathtrue" | "blindmathfalse" => {
            // Ignore blindtext math toggle.
        }
        "blindtext" => {
            output.push_str("#lorem(60)");
        }
        "blinditemize" => {
            output.push_str("\n- Lorem ipsum\n- Dolor sit amet\n- Consectetur\n");
        }
        "chapterprecis" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#quote[{}]", content.trim());
        }
        "AddToShipoutPicture" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&content);
            }
        }
        "ClearShipoutPicture" => {
            // Clear background layers; ignore in Typst.
        }
        "makeintropages" => {
            // UCLA intro pages macro; ignore.
        }
        "tableofcontents" | "maketableofcontents" | "thesistoc" => {
            output.push_str("\n#outline()\n");
        }
        "makeabstract" => {
            if let Some(abs) = conv.state.abstract_text.as_ref() {
                output.push_str("\n= Abstract\n\n");
                output.push_str(abs.trim());
                output.push('\n');
            }
        }
        "abstractpage" => {
            output.push_str("\n= Abstract\n\n");
        }
        "Abstract" => {
            output.push_str("\n= Abstract\n\n");
        }
        "acknowledgements" | "acknowledgments" | "thankpage" => {
            output.push_str("\n= Acknowledgments\n\n");
        }
        "acknowledgepage" => {
            output.push_str("\n= Acknowledgments\n\n");
        }
        "AgradecimentosAutorI" | "AgradecimentosAutorII" => {
            output.push_str("\n= Acknowledgments\n\n");
        }
        "dedicationpage" | "thesisDedication" | "Dedicatory" => {
            output.push_str("\n= Dedication\n\n");
        }
        "DedicatoriaAutorI" => {
            output.push_str("\n= Dedication\n\n");
        }
        "affidavit" => {
            output.push_str("\n= Affidavit\n\n");
        }
        "Declaration" => {
            output.push_str("\n= Declaration\n\n");
        }
        "Certificate" => {
            output.push_str("\n= Certificate\n\n");
        }
        "copyrightPage" | "thesiscopyrightpage" => {
            output.push_str("\n= Copyright\n\n");
        }
        "authorization" => {
            output.push_str("\n= Authorization\n\n");
        }
        "preface" => {
            output.push_str("\n= Preface\n\n");
        }
        "contentspage" | "maketoc" | "customtoc" | "thesistableofcontents" => {
            output.push_str("\n#outline()\n");
        }
        "listofsymbols" | "listsymbolname" | "printlosymbols" => {
            output.push_str("\n= List of Symbols\n\n");
        }
        "symbollist" => {
            output.push_str("\n= List of Symbols\n\n");
        }
        "listofacronyms" | "listofabbreviation" | "printloabbreviations" => {
            output.push_str("\n= Abbreviations\n\n");
        }
        "listofcontributions" => {
            output.push_str("\n= Contributions\n\n");
        }
        "listoffigandtable" => {
            output.push_str("\n= List of Figures and Tables\n\n");
        }
        "listofalgorithms" => {
            output.push_str("\n= List of Algorithms\n\n");
        }
        "addchap" => {
            if let Some(title) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "\n#heading(numbering: none)[{}]\n", title.trim());
            }
        }
        "approval" | "approvalpage" | "approvalPage" | "approvalsheet" => {
            output.push_str("\n= Approval\n\n");
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(content.trim());
                output.push('\n');
            }
        }
        "approvaldate" | "approvalDate" | "approvalStatement" | "approvalScan" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                if !content.trim().is_empty() {
                    output.push_str(content.trim());
                }
            }
        }
        "UDC" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let content = content.trim();
                if !content.is_empty() {
                    let _ = write!(output, "UDC: {}", content);
                }
            }
        }
        "Roman" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let trimmed = arg.trim();
                if let Ok(num) = trimmed.parse::<usize>() {
                    output.push_str(&to_roman_numeral(num));
                } else if !trimmed.is_empty() {
                    let _ = write!(output, "#counter(\"{}\").display(\"I\")", trimmed);
                }
            }
        }
        "roman" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let trimmed = arg.trim();
                if let Ok(num) = trimmed.parse::<usize>() {
                    output.push_str(&to_roman_numeral(num).to_lowercase());
                } else if !trimmed.is_empty() {
                    let _ = write!(output, "#counter(\"{}\").display(\"i\")", trimmed);
                }
            }
        }
        "thesisappendix" => {
            output.push_str("\n// Appendix\n");
            output.push_str("#counter(heading).update(0)\n");
            output.push_str("#set heading(numbering: \"A.\")\n\n");
        }
        "epigraph" => {
            let quote = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let author = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if !quote.trim().is_empty() {
                let _ = writeln!(output, "\n#quote[{}]\n", quote.trim());
            }
            if !author.trim().is_empty() {
                let _ = writeln!(output, "#align(right)[— {}]\n", author.trim());
            }
        }
        "iowaEpigraph" => {
            let quote = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let author = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if !quote.trim().is_empty() {
                let _ = writeln!(output, "\n#quote[{}]\n", quote.trim());
            }
            if !author.trim().is_empty() {
                let _ = writeln!(output, "#align(right)[— {}]\n", author.trim());
            }
        }
        "iflanguage" => {
            let then_branch = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let else_branch = conv.convert_required_arg(&cmd, 2).unwrap_or_default();
            let trimmed_then = then_branch.trim();
            if trimmed_then.is_empty() {
                output.push_str(else_branch.trim());
            } else {
                output.push_str(trimmed_then);
            }
        }
        "eject" => {
            output.push_str("\n#pagebreak()\n");
        }
        "dedication" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    output.push_str("\n= Dedication\n\n");
                    output.push_str(text.trim());
                    output.push('\n');
                }
            }
        }
        "subject" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Subject", text);
            }
        }
        "examiner" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Examiner", text);
            }
        }
        "cauthortitle" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Author title", text);
            }
        }
        "foreigntitle" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Foreign title", text);
            }
        }
        "thesistitle" | "mytitle" | "ctitle" | "etitle" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.state.title = Some(text.trim().to_string());
                    conv.push_thesis_meta("Title", text);
                }
            }
        }
        "timing" | "ENtiming" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    if conv.state.title.is_none() {
                        conv.state.title = Some(text.trim().to_string());
                    }
                    conv.push_thesis_meta("Title", text);
                }
            }
        }
        "futiming" | "ENfutiming" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Subtitle", text);
            }
        }
        "authornames" | "myname" | "cauthor" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                if !text.trim().is_empty() {
                    conv.state.author = Some(text.trim().to_string());
                    conv.push_thesis_meta("Author", text);
                }
            }
        }
        "zuozhexingming" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                if !text.trim().is_empty() {
                    conv.state.author = Some(text.trim().to_string());
                    conv.push_thesis_meta("Author", text);
                }
            }
        }
        "caffil" | "myinstitute" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Institution", text);
            }
        }
        "csubject" | "csubjecttitle" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Subject", text);
            }
        }
        "degreeprogram" | "univdegree" | "mytype" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree program", text);
            }
        }
        "professorship" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Professorship", text);
            }
        }
        "csupervisor" | "csupervisortitle" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Supervisor", text);
            }
        }
        "zhidaojiaoshi" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Advisor", text);
            }
        }
        "adviserone" | "advisertwo" | "advisert" | "advisertwot" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Advisor", text);
            }
        }
        "adviseronedegree" | "advisertwodegree" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Advisor degree", text);
            }
        }
        "authoronefirst" | "authoronefirstt" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Author 1 first", text);
            }
        }
        "authoronelast" | "authoronelastt" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Author 1 last", text);
            }
        }
        "authoroneex" | "authoroneextt" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Author 1 extra", text);
            }
        }
        "authortwofirst" | "authortwofirstt" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Author 2 first", text);
            }
        }
        "authortwolast" | "authortwolastt" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Author 2 last", text);
            }
        }
        "authortwoex" | "authortwoextt" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Author 2 extra", text);
            }
        }
        "cosupervisor" | "secondsupervisor" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Co-supervisor", text);
            }
        }
        "corrector" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Corrector", text);
            }
        }
        "committeemember" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_author_text(&raw);
                conv.push_thesis_meta("Committee member", text);
            }
        }
        "studentid" | "idnum" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Student ID", text);
            }
        }
        "xuehao" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Student ID", text);
            }
        }
        "submitdate" | "cdate" | "projectstart" | "projectend" | "timeend" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Date", text);
            }
        }
        "tijiaoriqi" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Submission date", text);
            }
        }
        "dabianriqi" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Defense date", text);
            }
        }
        "shouyudanwei" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree award", text);
            }
        }
        "xueweijibie" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Degree level", text);
            }
        }
        "zhuanye" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Major", text);
            }
        }
        "fenleihao" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Classification", text);
            }
        }
        "bianhao" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("School ID", text);
            }
        }
        "miji" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Confidentiality", text);
            }
        }
        "specialization" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Specialization", text);
            }
        }
        "mysection" | "mycurriculum" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Section", text);
            }
        }
        "thesistypeshort" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Thesis type", text);
            }
        }
        "thesisacronyms" => {
            output.push_str("\n= Acronyms\n\n");
        }
        "thesissymbols" => {
            output.push_str("\n= Symbols\n\n");
        }
        "date" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Date", text);
            }
        }
        "Year" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Year", text);
            }
        }
        "trnumber" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.push_thesis_meta("Report Number", text);
                }
            }
        }
        "committee" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                conv.push_thesis_meta("Committee", text);
            }
        }
        "support" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.push_thesis_meta("Support", text);
                }
            }
        }
        "disclaimer" => {
            if let Some(raw) = conv.get_required_arg_with_braces(&cmd, 0) {
                let text = super::utils::convert_caption_text(&raw);
                if !text.trim().is_empty() {
                    conv.push_thesis_meta("Disclaimer", text);
                }
            }
        }

        // Caption
        "caption" => {
            let content = conv.get_converted_required_arg(&cmd, 0).unwrap_or_default();
            match conv.state.current_env() {
                EnvironmentContext::Figure => {
                    let _ = write!(output, "  )\n  #figure.caption[{}]\n", content);
                }
                EnvironmentContext::Table => {
                    let _ = write!(output, "  ), caption: [{}]", content);
                }
                _ => {
                    let _ = write!(output, "[{}]", content);
                }
            }
        }
        "captionof" => {
            let kind = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let caption = conv.get_converted_required_arg(&cmd, 1).unwrap_or_default();
            if !caption.trim().is_empty() {
                let label = if kind.to_lowercase().contains("table") {
                    "Table"
                } else if kind.to_lowercase().contains("figure") {
                    "Figure"
                } else {
                    "Caption"
                };
                let _ = write!(output, "#block[*{}:* {}]", label, caption.trim());
            }
        }
        "resizebox" => {
            // \resizebox{w}{h}{content} -> drop sizing, keep content
            let content = conv.convert_required_arg(&cmd, 2).unwrap_or_default();
            if !content.trim().is_empty() {
                output.push_str(content.trim());
            }
        }
        "textquote" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                let _ = write!(output, "\"{}\"", content.trim());
            }
        }
        "arabic" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !content.trim().is_empty() {
                output.push_str(content.trim());
            }
        }

        // List item
        "item" => {
            output.push('\n');
            for _ in 0..conv.state.indent {
                output.push(' ');
            }
            let label_opt = conv
                .get_optional_arg(&cmd, 0)
                .or_else(|| extract_item_label_fallback(&cmd));
            let item_arg = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            match conv.state.current_env() {
                EnvironmentContext::Enumerate => {
                    // Check for optional label
                    if let Some(label) = label_opt.as_ref() {
                        let label = convert_caption_text(label);
                        let _ = write!(output, "+ [{}] ", label);
                    } else {
                        output.push_str("+ ");
                    }
                }
                EnvironmentContext::Description => {
                    if let Some(term) = label_opt.as_ref() {
                        let term = convert_caption_text(term);
                        if term.trim().is_empty() {
                            output.push_str("- ");
                        } else {
                            let _ = write!(output, "/ {}: ", term);
                        }
                    } else {
                        output.push_str("- ");
                    }
                }
                _ => {
                    output.push_str("- ");
                }
            }
            let item_trim = item_arg.trim();
            let is_empty_math = matches!(item_trim, "$" | "$$");
            if !item_trim.is_empty() && !is_empty_math {
                output.push_str(item_trim);
                output.push(' ');
            }
        }

        // Math operators (in math mode)
        // \operatorname and \operatorname* - handled via pending_op state machine
        "operatorname" | "operatorname*" => {
            let is_limits = base_name.ends_with('*')
                || cmd
                    .syntax()
                    .text()
                    .to_string()
                    .starts_with("\\operatorname*");
            // Try to get the argument (if parsed as part of the command)
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                // Clean up: remove spaces
                let clean_content: String =
                    content.chars().filter(|c| !c.is_whitespace()).collect();

                let op_name = if clean_content == "argmin" {
                    "argmin"
                } else if clean_content == "argmax" {
                    "argmax"
                } else {
                    &clean_content
                };

                if is_limits {
                    let _ = write!(output, "limits(op(\"{}\")) ", op_name);
                } else {
                    let _ = write!(output, "op(\"{}\") ", op_name);
                }
            } else {
                // Argument not captured, set pending state for next ItemCurly
                conv.state.pending_op = Some(PendingOperator { is_limits });
            }
        }

        // Math fractions
        "sfrac" => {
            let num = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let den = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let num = num.trim();
            let den = den.trim();
            if num.is_empty() || den.is_empty() {
                return;
            }

            let expr = if conv.state.options.frac_to_slash
                && conv.is_simple_term(num)
                && conv.is_simple_term(den)
            {
                format!("{}/{}", num, den)
            } else {
                format!("inline(frac({}, {}))", num, den)
            };

            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push_str(&expr);
                output.push(' ');
            } else {
                let _ = write!(output, "$ {} $", expr);
            }
        }
        "frac" => {
            let num = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let den = conv.convert_required_arg(&cmd, 1).unwrap_or_default();

            // Check if we can use slash notation
            if conv.state.options.frac_to_slash
                && conv.is_simple_term(&num)
                && conv.is_simple_term(&den)
            {
                let _ = write!(output, "{}/{} ", num.trim(), den.trim());
            } else {
                let _ = write!(output, "frac({}, {})", num.trim(), den.trim());
            }
        }
        "dfrac" => {
            // dfrac always uses frac() for proper display style
            let num = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let den = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let _ = write!(output, "display(frac({}, {}))", num.trim(), den.trim());
        }
        "tfrac" => {
            // tfrac might use slash if enabled and simple
            let num = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let den = conv.convert_required_arg(&cmd, 1).unwrap_or_default();

            if conv.state.options.frac_to_slash
                && conv.is_simple_term(&num)
                && conv.is_simple_term(&den)
            {
                let _ = write!(output, "{}/{} ", num.trim(), den.trim());
            } else {
                let _ = write!(output, "inline(frac({}, {}))", num.trim(), den.trim());
            }
        }
        "prescript" => {
            // \prescript{pre-sup}{pre-sub}{base} -> _{pre-sub}^{pre-sup} base
            let sup = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let sub = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let base = conv.convert_required_arg(&cmd, 2).unwrap_or_default();

            let sup = sup.trim();
            let sub = sub.trim();
            let base = base.trim();
            if base.is_empty() {
                return;
            }

            let mut expr = String::new();
            if !sub.is_empty() {
                let _ = write!(expr, "_{{{}}}", sub);
            }
            if !sup.is_empty() {
                let _ = write!(expr, "^{{{}}}", sup);
            }
            if !expr.is_empty() {
                expr.push(' ');
            }
            expr.push_str(base);

            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push_str(&expr);
                output.push(' ');
            } else {
                let _ = write!(output, "$ {} $", expr);
            }
        }
        "cfrac" => {
            let num = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let den = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let _ = write!(output, "frac({}, {})", num.trim(), den.trim());
        }

        // Math roots
        "sqrt" => {
            let opt = conv.get_optional_arg(&cmd, 0);
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let protected = protect_comma(&content);
            if let Some(n) = opt {
                let _ = write!(output, "root({}, {})", n, protected);
            } else {
                let _ = write!(output, "sqrt({})", protected);
            }
        }

        // Math accents and decorations (with argument)
        "hat" | "widehat" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "hat({}) ", arg);
            }
        }
        "tilde" | "widetilde" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "tilde({}) ", arg);
            }
        }
        "bar" | "overline" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "overline({}) ", arg);
            }
        }
        "vec" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "arrow({}) ", arg);
            }
        }
        "dot" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "dot({}) ", arg);
            }
        }
        "overbrace" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "overbrace({}) ", arg);
            }
        }
        "underbrace" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "underbrace({}) ", arg);
            }
        }
        "overleftarrow" | "overrightarrow" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "arrow({}) ", arg);
            }
        }
        "overleftrightarrow" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "arrow({}) ", arg);
            }
        }
        "rlap" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(arg.trim());
                output.push(' ');
            }
        }
        "vertiii" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                if conv.state.mode == ConversionMode::Math {
                    let _ = write!(output, "norm({}) ", arg);
                } else {
                    let _ = write!(output, "$ norm({}) $", arg);
                }
            }
        }
        "quark" => {
            if conv.state.mode == ConversionMode::Math {
                output.push_str("dot ");
            } else {
                output.push_str("dot");
            }
        }
        "rd" => {
            if conv.state.mode == ConversionMode::Math {
                output.push_str("upright(d) ");
            } else {
                output.push('d');
            }
        }
        "mevcc" | "gevcc" | "gevc" | "tev" => {
            let expr = match base_name {
                "mevcc" => "upright(\"MeV\") / c^2",
                "gevcc" => "upright(\"GeV\") / c^2",
                "gevc" => "upright(\"GeV\") / c",
                "tev" => "upright(\"TeV\")",
                _ => "",
            };
            if expr.is_empty() {
                return;
            }
            if conv.state.mode == ConversionMode::Math {
                output.push_str(expr);
                output.push(' ');
            } else {
                let _ = write!(output, "$ {} $", expr);
            }
        }
        "Bbar" => {
            let expr = "overline(B)";
            if conv.state.mode == ConversionMode::Math {
                output.push_str(expr);
                output.push(' ');
            } else {
                let _ = write!(output, "$ {} $", expr);
            }
        }
        "ddot" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "dot.double({}) ", arg);
            }
        }
        "mathbf" => {
            // \mathbf{x} -> upright(bold(x)) for proper bold upright
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                // Replace commas with 'comma' symbol to avoid argument separator issues
                let safe_content = content.replace(',', " comma ");
                let _ = write!(output, "upright(bold({})) ", safe_content);
            }
        }
        "boldsymbol" | "bm" => {
            // \boldsymbol and \bm just use bold()
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                // Replace commas with 'comma' symbol to avoid argument separator issues
                let safe_content = content.replace(',', " comma ");
                let _ = write!(output, "bold({}) ", safe_content);
            }
        }
        "mathit" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                // Replace commas with 'comma' symbol to avoid argument separator issues
                let safe_content = content.replace(',', " comma ");
                let _ = write!(output, "italic({}) ", safe_content);
            }
        }
        "mathrm" => {
            // Check for special case: \mathrm{d} -> dif (differential)
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                if content.trim() == "d" || content.trim() == "dif" {
                    output.push_str("dif ");
                } else {
                    // Replace commas with 'comma' symbol to avoid argument separator issues
                    let safe_content = content.replace(',', " comma ");
                    let _ = write!(output, "upright({}) ", safe_content);
                }
            }
        }
        "rm" => {
            // \rm is an old-style font switch (no braces)
            if let Some(content) = conv.get_required_arg(&cmd, 0) {
                // Replace commas with 'comma' symbol to avoid argument separator issues
                let safe_content = content.replace(',', " comma ");
                let _ = write!(output, "upright({}) ", safe_content);
            }
            // If no argument, just skip
        }
        "Bbbk" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push_str("bb(k) ");
            } else {
                output.push_str("$ bb(k) $");
            }
        }
        "mathbb" | "mathds" | "mathbbm" => {
            if let Some(content) = conv.get_required_arg(&cmd, 0) {
                let content = content.trim();
                // Only use short forms for standard number sets that Typst supports as symbols
                if ["R", "Z", "N", "C", "Q"].contains(&content) {
                    let c = content.chars().next().unwrap();
                    let _ = write!(output, "{}{} ", c, c);
                } else {
                    let _ = write!(output, "bb({}) ", content);
                }
            }
        }
        "matSpace" | "polMatSpace" | "matSpaceAux" | "polMatSpaceAux" => {
            let first = conv.get_optional_arg(&cmd, 0).unwrap_or_default();
            let second = conv.get_optional_arg(&cmd, 1).unwrap_or_default();
            let a = if first.trim().is_empty() {
                String::new()
            } else {
                super::latex_math_to_typst(first.trim())
            };
            let b = if second.trim().is_empty() {
                String::new()
            } else {
                super::latex_math_to_typst(second.trim())
            };
            let base = if base_name.contains("polMatSpace") {
                "bb(K)[X]"
            } else {
                "bb(K)"
            };
            if !a.is_empty() && !b.is_empty() {
                let _ = write!(output, "{}^({} times {}) ", base, a.trim(), b.trim());
            } else if !a.is_empty() {
                let _ = write!(output, "{}^({}) ", base, a.trim());
            } else {
                output.push_str(base);
                output.push(' ');
            }
        }
        "mathcal" | "symcal" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "cal({}) ", content);
            }
        }
        "cal" => {
            // Old-style \cal switch (often used as \cal A)
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "cal({}) ", content.trim());
            }
        }
        "pazocal" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "cal({}) ", content.trim());
            }
        }
        "mathfrak" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "frak({}) ", content);
            }
        }
        "mathsf" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "sans({}) ", content);
            }
        }
        "mathtt" => {
            // \mathtt{} in math mode is typically for code/identifiers that should stay together
            // Get raw text and output as mono("...") string, not as separated math variables
            if let Some(raw) = conv.get_required_arg(&cmd, 0) {
                let text = convert_caption_text(raw.trim());
                let escaped = escape_typst_string(&text);
                let _ = write!(output, "mono(\"{}\") ", escaped);
            }
        }
        "abs" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "|{}| ", content);
            }
        }
        "norm" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "||{}|| ", content);
            }
        }
        "floor" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "⌊{}⌋ ", content);
            }
        }
        "binom" => {
            let a = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let b = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if !a.trim().is_empty() || !b.trim().is_empty() {
                let _ = write!(output, "binom({}, {}) ", a.trim(), b.trim());
            }
        }
        "lefteqn" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(content.trim());
                output.push(' ');
            }
        }
        "mathscr" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "scr({}) ", content);
            }
        }
        "cancel" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "cancel({})", content.trim());
        }
        // Boxed content - handle differently in math vs text mode
        "boxed" | "fbox" | "framebox" => {
            let arg = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if conv.state.mode == ConversionMode::Math {
                // In math mode, wrap with $...$ for math content
                let _ = write!(
                    output,
                    "#box(stroke: 0.5pt, inset: 2pt, baseline: 20%)[$ {} $] ",
                    arg.trim()
                );
            } else {
                // In text mode, output directly without math wrapper
                let _ = write!(
                    output,
                    "#box(stroke: 0.5pt, inset: 2pt)[{}] ",
                    arg.trim()
                );
            }
        }

        // Equation tag (custom numbering)
        "tag" | "tag*" => {
            // \tag{label} - custom equation number
            // In Typst, we can simulate this with right-aligned text
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                // Use #h(1fr) to push to the right, wrap in parentheses
                let _ = write!(output, " #h(1fr) \"({})\"", content.trim());
            }
        }

        // siunitx commands
        "SI" => {
            // \SI{value}{unit} - old siunitx syntax
            let value = conv.get_required_arg(&cmd, 0);
            let unit = conv.get_required_arg(&cmd, 1);
            match (value, unit) {
                (Some(v), Some(u)) => {
                    let unit_str = conv.process_si_unit(&u);
                    let _ = write!(output, "${} space {}$", v, unit_str);
                }
                (Some(v), None) => {
                    let _ = write!(output, "${}$", v);
                }
                _ => {}
            }
        }
        "si" => {
            // \si{unit} - unit only
            if let Some(unit) = conv.get_required_arg(&cmd, 0) {
                let unit_str = conv.process_si_unit(&unit);
                let _ = write!(output, "${}$", unit_str);
            }
        }
        "qty" => {
            let value = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let unit = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            let unit_str = conv.process_si_unit(&unit);
            let _ = write!(output, "${} space {}$", value, unit_str);
        }
        "num" => {
            let value = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "${}$", value);
        }
        "unit" => {
            let unit = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let unit_str = conv.process_si_unit(&unit);
            let _ = write!(output, "${}$", unit_str);
        }
        "ang" => {
            let angle = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "${}°$", angle);
        }

        // Acronym commands - auto (first use = full, subsequent = short)
        "ac" | "gls" | "Ac" | "Gls" | "GLS" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some((text, _is_first)) = conv.state.use_acronym(&key) {
                let text = if base_name.starts_with('G') || base_name.starts_with("Ac") {
                    let mut chars = text.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => text,
                    }
                } else {
                    text
                };
                output.push_str(&text);
            } else if let Some(name) = conv.state.get_glossary_name(&key) {
                output.push_str(&name);
            } else {
                output.push_str(&key);
            }
        }
        // Acronym commands - plural forms
        "glspl" | "acp" | "Glspl" | "Acp" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(acr) = conv.state.acronyms.get(&key) {
                let plural = acr.short_plural();
                let text = if base_name.starts_with('G') || base_name.starts_with("Ac") {
                    let mut chars = plural.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => plural,
                    }
                } else {
                    plural
                };
                output.push_str(&text);
            } else {
                output.push_str(&key);
                output.push('s');
            }
        }
        // Acronym commands - short form only
        "acs" | "acrshort" | "Acs" | "Acrshort" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(short) = conv.state.get_acronym_short(&key) {
                let text = if base_name.starts_with("Acs") || base_name.starts_with("Acr") && base_name.chars().nth(3) == Some('s') {
                    let mut chars = short.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => short,
                    }
                } else {
                    short
                };
                output.push_str(&text);
            } else {
                output.push_str(&key);
            }
        }
        // Acronym commands - long form only
        "acl" | "acrlong" | "Acl" | "Acrlong" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(long) = conv.state.get_acronym_long(&key) {
                let text = if base_name.starts_with("Acl") || base_name.starts_with("Acr") && base_name.chars().nth(3) == Some('l') {
                    let mut chars = long.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => long,
                    }
                } else {
                    long
                };
                output.push_str(&text);
            } else {
                output.push_str(&key);
            }
        }
        // Acronym commands - full form (always)
        "acf" | "acrfull" | "Acf" | "Acrfull" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(full) = conv.state.get_acronym_full(&key) {
                let text = if base_name.starts_with("Acf") || base_name.starts_with("Acr") && base_name.chars().nth(3) == Some('f') {
                    let mut chars = full.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => full,
                    }
                } else {
                    full
                };
                output.push_str(&text);
            } else {
                output.push_str(&key);
            }
        }
        // Glossary description
        "glsdesc" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(name) = conv.state.get_glossary_name(&key) {
                output.push_str(&name);
            } else if let Some(long) = conv.state.get_acronym_long(&key) {
                output.push_str(&long);
            } else {
                output.push_str(&key);
            }
        }
        // Plural full/short/long forms
        "acfp" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(acr) = conv.state.acronyms.get(&key) {
                output.push_str(&acr.full_plural());
            } else {
                output.push_str(&key);
                output.push('s');
            }
        }
        "acsp" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(acr) = conv.state.acronyms.get(&key) {
                output.push_str(&acr.short_plural());
            } else {
                output.push_str(&key);
                output.push('s');
            }
        }
        "aclp" => {
            let key = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(acr) = conv.state.acronyms.get(&key) {
                output.push_str(&acr.long_plural());
            } else {
                output.push_str(&key);
                output.push('s');
            }
        }

        // Spacing commands
        "hspace" | "hspace*" | "hskip" => {
            let dim = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if !dim.trim().is_empty() {
                let _ = write!(output, "#h({})", convert_dimension(&dim));
            }
        }
        "vspace" | "vspace*" | "vskip" => {
            let dim = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            if !dim.trim().is_empty() {
                let _ = write!(output, "#v({})", convert_dimension(&dim));
            }
        }
        "afterpage" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&content);
            }
        }
        "put" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&content);
            }
        }
        "mspace" => {
            let _ = conv.get_required_arg(&cmd, 0);
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push(' ');
            }
        }
        "quad" => output.push_str("  "),
        "qquad" => output.push_str("    "),
        "," | "thinspace" => output.push(' '),
        ";" | "thickspace" => output.push_str("  "),
        "!" | "negthinspace" => {}
        "enspace" => output.push(' '),

        // Line breaks
        "newline" | "linebreak" => {
            output.push_str("\\ ");
        }
        "par" | "bigskip" | "medskip" | "smallskip" => {
            output.push_str("\n\n");
        }

        // Special math symbols
        "infty" => {
            if conv.state.options.infty_to_oo {
                output.push_str("oo");
            } else {
                output.push_str("infinity");
            }
        }

        // Special characters
        "LaTeX" | "latex" => output.push_str("LaTeX"),
        "TeX" => output.push_str("TeX"),
        "today" => output.push_str("#datetime.today().display()"),
        "eg" => output.push_str("e.g."),
        "ie" => output.push_str("i.e."),
        "etal" => output.push_str("et al."),
        "vs" => output.push_str("vs."),
        "ldots" | "dots" | "cdots" => output.push_str("..."),
        "copyright" => output.push('©'),
        "trademark" | "texttrademark" => output.push('™'),
        "registered" | "textregistered" => output.push('®'),
        "dag" | "dagger" => output.push('†'),
        "ddag" | "ddagger" => output.push('‡'),
        "S" => output.push('§'),
        "P" => output.push('¶'),
        "pounds" | "textsterling" => output.push('£'),
        "euro" => output.push('€'),
        "textbackslash" | "backslash" => output.push('\\'),
        "textasciitilde" => output.push('~'),
        "textasciicircum" => output.push('^'),
        "nobreakspace" => output.push('~'),
        "textasciigrave" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push('`');
            } else {
                output.push_str("\\`");
            }
        }
        "%" => output.push('%'),
        "&" => output.push('&'),
        "$" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push('$');
            } else {
                output.push_str("\\$");
            }
        }
        "#" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push('#');
            } else {
                output.push_str("\\#");
            }
        }
        "_" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push('_');
            } else {
                output.push_str("\\_");
            }
        }
        "*" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push('*');
            } else {
                output.push_str("\\*");
            }
        }
        "@" => {} // \@ in LaTeX is spacing control - strip it
        "{" => output.push('{'),
        "}" => output.push('}'),

        // Accents in text mode
        "`" => {
            // grave accent
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, '`'));
        }
        "'" => {
            // acute accent
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, '\''));
        }
        "^" => {
            // circumflex
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, '^'));
        }
        "~" => {
            // tilde
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, '~'));
        }
        "\"" => {
            // umlaut
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, '"'));
        }
        "c" => {
            // cedilla
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_cedilla(&content));
        }
        "v" => {
            // caron
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, 'v'));
        }
        "u" => {
            // breve
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, 'u'));
        }
        "k" => {
            // ogonek
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, 'k'));
        }
        "H" => {
            // double acute
            let content = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&apply_text_accent(&content, 'H'));
        }

        // Color commands (using parse_color_expression for proper color mapping)
        "textcolor" => {
            let color = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let typst_color = resolve_color_expression(conv, &color);
            let _ = write!(output, "#text(fill: {})[{}]", typst_color, content);
        }
        "colorbox" => {
            let color = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let typst_color = resolve_color_expression(conv, &color);
            let _ = write!(output, "#box(fill: {}, inset: 2pt)[{}]", typst_color, content);
        }
        "fcolorbox" => {
            let frame_color = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let bg_color = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            let content = conv.convert_required_arg(&cmd, 2).unwrap_or_default();
            let typst_frame = resolve_color_expression(conv, &frame_color);
            let typst_bg = resolve_color_expression(conv, &bg_color);
            let _ = write!(
                output,
                "#box(fill: {}, stroke: {}, inset: 2pt)[{}]",
                typst_bg, typst_frame, content
            );
        }
        "highlight" | "hl" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "#highlight[{}]", content);
        }

        // Math limits and bounds - add trailing space to prevent merging (e.g. \arg\min -> "arg min")
        "lim" => output.push_str("lim "),
        "sup" => output.push_str("sup "),
        "inf" => output.push_str("inf "),
        "max" => {
            if !super::utils::merge_arg_operator(output, "max") {
                output.push_str("max ");
            }
        }
        "min" => {
            if !super::utils::merge_arg_operator(output, "min") {
                output.push_str("min ");
            }
        }
        "arg" => output.push_str("arg "),
        "det" => output.push_str("det "),
        "gcd" => output.push_str("gcd "),
        "lcm" => output.push_str("op(\"lcm\") "),
        "log" => output.push_str("log "),
        "ln" => output.push_str("ln "),
        "lg" => output.push_str("lg "),
        "exp" => output.push_str("exp "),
        "sin" => output.push_str("sin "),
        "cos" => output.push_str("cos "),
        "tan" => output.push_str("tan "),
        "cot" => output.push_str("cot "),
        "sec" => output.push_str("sec "),
        "csc" => output.push_str("csc "),
        "sinh" => output.push_str("sinh "),
        "cosh" => output.push_str("cosh "),
        "tanh" => output.push_str("tanh "),
        "coth" => output.push_str("coth "),
        "arcsin" => output.push_str("arcsin "),
        "arccos" => output.push_str("arccos "),
        "arctan" => output.push_str("arctan "),
        "Pr" => output.push_str("op(\"Pr\") "),
        "hom" => output.push_str("hom "),
        "ker" => output.push_str("ker "),
        "dim" => output.push_str("dim "),
        "deg" => output.push_str("deg "),

        // Big operators - trailing space prevents merging with following content
        "sum" => output.push_str("sum "),
        "prod" | "product" => output.push_str("product "),
        "limits" => {
            let _ = super::utils::apply_limits_to_trailing_operator(output);
        }
        "nolimits" => {
            // Limits control; ignore and rely on Typst defaults.
        }
        "int" => output.push_str("integral "),
        "iint" => output.push_str("integral.double "),
        "iiint" => output.push_str("integral.triple "),
        "oint" => output.push_str("integral.cont "),
        "bigcup" => output.push_str("union.big "),
        "bigcap" => output.push_str("sect.big "),
        "bigoplus" => output.push_str("plus.circle.big "),
        "bigotimes" => output.push_str("times.circle.big "),
        "bigsqcup" => output.push_str("union.sq.big "),
        "biguplus" => output.push_str("union.plus.big "),
        "bigvee" => output.push_str("or.big "),
        "bigwedge" => output.push_str("and.big "),
        "coprod" => output.push_str("product.co "),

        // Delimiters
        "left" | "right" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl"
        | "Biggr" | "middle" => {
            // These are handled by ItemLR
        }

        // Phantom and spacing - in math mode, use #hide() since hide() alone isn't a math function
        "phantom" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if conv.state.mode == ConversionMode::Math {
                let _ = write!(output, "#hide[$ {} $]", content.trim());
            } else {
                let _ = write!(output, "#hide[{}]", content);
            }
        }
        "hphantom" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if conv.state.mode == ConversionMode::Math {
                let _ = write!(output, "#hide[$ {} $]", content.trim());
            } else {
                let _ = write!(output, "#hide[{}]", content);
            }
        }
        "vphantom" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if conv.state.mode == ConversionMode::Math {
                let _ = write!(output, "#hide[$ {} $]", content.trim());
            } else {
                let _ = write!(output, "#hide[{}]", content);
            }
        }

        // Stacking - tex2typst style with limits()
        "overset" => {
            // \overset{top}{base} -> limits(base)^(top)
            // Special optimization: \overset{\text{def}}{=} -> eq.def
            let top = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let base = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let top_trimmed = top.trim().replace("\"", "");
            if (top_trimmed == "def" || top_trimmed.contains("def"))
                && (base.trim() == "=" || base.trim() == "eq")
            {
                output.push_str("eq.def ");
            } else {
                if let Some(base_wrapped) = super::utils::wrap_with_limits_for_stack(&base) {
                    if top.trim().is_empty() {
                        let _ = write!(output, "{} ", base_wrapped);
                    } else {
                        let _ = write!(output, "{}^({}) ", base_wrapped, top);
                    }
                } else if !top.trim().is_empty() {
                    let _ = write!(output, "{} ", top.trim());
                }
            }
        }
        "underset" => {
            // \underset{bottom}{base} -> limits(base)_(bottom)
            let bottom = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let base = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if let Some(base_wrapped) = super::utils::wrap_with_limits_for_stack(&base) {
                if bottom.trim().is_empty() {
                    let _ = write!(output, "{} ", base_wrapped);
                } else {
                    let _ = write!(output, "{}_({}) ", base_wrapped, bottom);
                }
            } else if !bottom.trim().is_empty() {
                let _ = write!(output, "{} ", bottom.trim());
            }
        }
        "stackrel" => {
            // \stackrel{top}{relation} -> limits(relation)^(top)
            let top = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let base = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            if let Some(base_wrapped) = super::utils::wrap_with_limits_for_stack(&base) {
                if top.trim().is_empty() {
                    let _ = write!(output, "{} ", base_wrapped);
                } else {
                    let _ = write!(output, "{}^({}) ", base_wrapped, top);
                }
            } else if !top.trim().is_empty() {
                let _ = write!(output, "{} ", top.trim());
            }
        }
        "substack" => {
            // \substack{a \\ b} -> directly output content
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&arg);
            }
        }

        // Protect / misc
        "protect" => {
            // ignore
        }
        "mbox" | "makebox" | "hbox" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            let _ = write!(output, "\"{}\"", content);
        }
        "raisebox" => {
            let _height = conv.get_required_arg(&cmd, 0);
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            output.push_str(&content);
        }
        "parbox" => {
            let _width = conv.get_required_arg(&cmd, 0);
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            output.push_str(&content);
        }
        "minipage" => {
            let content = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            output.push_str(&content);
        }

        // Table commands
        "hline" | "toprule" | "midrule" | "bottomrule" => {
            output.push_str("|||HLINE|||");
        }
        "hhline" => {
            output.push_str("|||HLINE|||");
        }
        "cline" | "cmidrule" => {
            output.push_str("|||HLINE|||");
        }
        "tabularnewline" | "cr" => match conv.state.current_env() {
            EnvironmentContext::Tabular => output.push_str("|||ROW|||"),
            _ => output.push_str("\\ "),
        },
        "multicolumn" => {
            let ncols = conv.get_required_arg(&cmd, 0).unwrap_or("1".to_string());
            let _align = conv.get_required_arg(&cmd, 1);
            let content = conv.convert_required_arg(&cmd, 2).unwrap_or_default();
            let _ = write!(output, "___TYPST_CELL___:table.cell(colspan: {})[{}]", ncols, content);
        }
        "multirow" => {
            let nrows = conv.get_required_arg(&cmd, 0).unwrap_or("1".to_string());
            let width_raw = conv.get_required_arg_with_braces(&cmd, 1);
            let content_idx = match width_raw.as_deref() {
                Some(w) if w.trim() == "*" => 1,
                _ => 2,
            };
            let content = conv.convert_required_arg(&cmd, content_idx).unwrap_or_default();
            let _ = write!(output, "___TYPST_CELL___:table.cell(rowspan: {})[{}]", nrows, content);
        }
        "multirowcell" => {
            let nrows = conv.get_required_arg(&cmd, 0).unwrap_or("1".to_string());
            let content = conv.convert_required_arg(&cmd, 1).unwrap_or_default();
            let _ = write!(output, "___TYPST_CELL___:table.cell(rowspan: {})[{}]", nrows, content);
        }

        // Extensible arrows with text above/below
        "xleftarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.l.long)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.l.long)^({}) ", above);
            }
        }
        "xrightarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.r.long)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.r.long)^({}) ", above);
            }
        }
        "xmapsto" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.r.long.bar)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.r.long.bar)^({}) ", above);
            }
        }
        "xleftrightarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.l.r.long)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.l.r.long)^({}) ", above);
            }
        }
        "xLeftarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.l.double.long)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.l.double.long)^({}) ", above);
            }
        }
        "xRightarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.r.double.long)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.r.double.long)^({}) ", above);
            }
        }
        "xLeftrightarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.l.r.double.long)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.l.r.double.long)^({}) ", above);
            }
        }
        "xhookleftarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.l.hook)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.l.hook)^({}) ", above);
            }
        }
        "xhookrightarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.r.hook)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.r.hook)^({}) ", above);
            }
        }
        "xtwoheadleftarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.l.twohead)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.l.twohead)^({}) ", above);
            }
        }
        "xtwoheadrightarrow" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrow.r.twohead)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrow.r.twohead)^({}) ", above);
            }
        }
        "xleftharpoonup" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(harpoon.lt)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(harpoon.lt)^({}) ", above);
            }
        }
        "xrightharpoonup" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(harpoon.rt)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(harpoon.rt)^({}) ", above);
            }
        }
        "xleftharpoondown" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(harpoon.lb)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(harpoon.lb)^({}) ", above);
            }
        }
        "xrightharpoondown" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(harpoon.rb)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(harpoon.rb)^({}) ", above);
            }
        }
        "xleftrightharpoons" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(harpoons.ltrb)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(harpoons.ltrb)^({}) ", above);
            }
        }
        "xrightleftharpoons" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(harpoons.rtlb)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(harpoons.rtlb)^({}) ", above);
            }
        }
        "xtofrom" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(arrows.rl)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(arrows.rl)^({}) ", above);
            }
        }
        "xlongequal" => {
            let below = conv.get_optional_arg(&cmd, 0);
            let above = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if let Some(b) = below {
                let _ = write!(output, "limits(eq.triple)^({})_({}) ", above, b);
            } else {
                let _ = write!(output, "limits(eq.triple)^({}) ", above);
            }
        }

        // Modular arithmetic
        "bmod" => output.push_str("mod "),
        "pmod" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "(mod {}) ", arg);
            }
        }
        "pod" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "({}) ", arg);
            }
        }

        // Math class commands (spacing/classification)
        "mathrel" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "relation", &arg);
            }
        }
        "mathbin" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "binary", &arg);
            }
        }
        "mathop" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "large", &arg);
            }
        }
        "mathord" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "normal", &arg);
            }
        }
        "mathopen" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "opening", &arg);
            }
        }
        "mathclose" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "closing", &arg);
            }
        }
        "mathpunct" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                write_math_class(output, "punctuation", &arg);
            }
        }
        "mathinner" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&arg);
                output.push(' ');
            }
        }

        // Displaylines
        "displaylines" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(&arg);
            }
        }

        // Set notation (braket package)
        "set" | "Set" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "{{ {} }} ", arg);
            }
        }
        "ket" | "Ket" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "|{}⟩ ", arg.trim());
            }
        }
        "bra" | "Bra" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "⟨{}| ", arg.trim());
            }
        }
        "braket" | "Braket" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "⟨{}⟩ ", arg.trim());
            }
        }

        // Comparison aliases (with shorthand support)
        "ne" | "neq" => {
            let sym = apply_shorthand("eq.not", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "le" | "leq" => {
            let sym = apply_shorthand("lt.eq", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "ge" | "geq" => {
            let sym = apply_shorthand("gt.eq", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }

        // Common math operators/symbols
        "times" => output.push_str("times "),
        "cdot" => output.push_str("dot "),
        "div" => output.push_str("div "),
        "pm" => output.push_str("plus.minus "),
        "mp" => output.push_str("minus.plus "),
        "ast" => output.push_str("ast "),
        "star" => output.push_str("star "),
        "circ" => output.push_str("circle.small "),
        "bullet" => output.push_str("bullet "),
        "over" => {
            if conv.state.mode == ConversionMode::Math {
                output.push_str("/ ");
            }
        }
        "choose" => {
            if conv.state.mode == ConversionMode::Math {
                output.push_str("choose ");
            }
        }

        // Arrows (with shorthand support)
        "rightarrow" | "to" => {
            let sym = apply_shorthand("arrow.r", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "leftarrow" => {
            let sym = apply_shorthand("arrow.l", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "leftrightarrow" => {
            let sym = apply_shorthand("arrow.l.r", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "Rightarrow" | "implies" => {
            let sym = apply_shorthand("arrow.r.double", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "Leftarrow" => {
            let sym = apply_shorthand("arrow.l.double", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "Leftrightarrow" | "iff" => {
            let sym = apply_shorthand("arrow.l.r.double", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "mapsto" => {
            let sym = apply_shorthand("arrow.r.bar", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "uparrow" => output.push_str("arrow.t "),
        "downarrow" => output.push_str("arrow.b "),
        "nint" => output.push_str("∫ "),
        "dint" => output.push_str("∬ "),

        // Definition/equality variants
        "coloneqq" => {
            let sym = apply_shorthand("colon.eq", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "eqqcolon" => {
            let sym = apply_shorthand("eq.colon", conv.state.options.prefer_shorthands);
            let _ = write!(output, "{} ", sym);
        }
        "Coloneqq" => output.push_str("colon.double.eq "),
        "Eqqcolon" => output.push_str("eq.colon "),

        // Set operations
        "in" => output.push_str("in "),
        "notin" => output.push_str("in.not "),
        "subset" => output.push_str("subset "),
        "subseteq" => output.push_str("subset.eq "),
        "supset" => output.push_str("supset "),
        "supseteq" => output.push_str("supset.eq "),
        "cup" => output.push_str("union "),
        "cap" => output.push_str("sect "),
        "emptyset" | "varnothing" => output.push_str("emptyset "),
        "uplus" => output.push_str("⊎ "),
        "llbracket" => output.push_str("⟦ "),
        "rrbracket" => output.push_str("⟧ "),

        // Logic
        "land" | "wedge" => output.push_str("and "),
        "lor" | "vee" => output.push_str("or "),
        "lnot" | "neg" => output.push_str("not "),
        "forall" => output.push_str("forall "),
        "exists" => output.push_str("exists "),

        // Relations
        "approx" => output.push_str("approx "),
        "sim" => output.push_str("tilde.op "),
        "simeq" => output.push_str("tilde.eq "),
        "cong" => output.push_str("tilde.equiv "),
        "equiv" => output.push_str("equiv "),
        "propto" => output.push_str("prop "),
        "parallel" => output.push_str("parallel "),
        "perp" => output.push_str("perp "),

        // Dots
        "vdots" => output.push_str("dots.v "),
        "ddots" => output.push_str("dots.down "),
        "hdots" => output.push_str("dots.h "),

        // Misc symbols
        "partial" => output.push_str("partial "),
        "nabla" => output.push_str("nabla "),
        "prime" => output.push_str("prime "),
        "degree" => output.push_str("degree "),
        "angle" => output.push_str("angle "),
        "ell" => output.push_str("ell "),
        "hbar" => output.push_str("planck.reduce "),
        "upmu" => output.push_str("mu "),
        "Re" => output.push_str("Re "),
        "Im" => output.push_str("Im "),
        "wp" => output.push_str("wp "),
        "aleph" => output.push_str("aleph "),
        "beth" => output.push_str("beth "),
        "gimel" => output.push_str("gimel "),

        // Additional integrals
        "iiiint" => output.push_str("integral.quad "),
        "oiint" => output.push_str("integral.surf "),
        "oiiint" => output.push_str("integral.vol "),

        // Additional limits
        "liminf" => output.push_str("liminf "),
        "limsup" => output.push_str("limsup "),
        "injlim" => output.push_str("op(\"inj lim\")"),
        "projlim" => output.push_str("op(\"proj lim\")"),
        "varinjlim" => output.push_str("underline(lim, arrow.r) "),
        "varprojlim" => output.push_str("underline(lim, arrow.l) "),
        "mod" => output.push_str("mod "),

        // Brackets and delimiters
        "langle" => output.push_str("chevron.l "),
        "rangle" => output.push_str("chevron.r "),
        "lfloor" => output.push_str("floor.l "),
        "rfloor" => output.push_str("floor.r "),
        "lceil" => output.push_str("ceil.l "),
        "rceil" => output.push_str("ceil.r "),
        "lvert" => output.push_str("bar.v "),
        "rvert" => output.push_str("bar.v "),
        "lVert" => output.push_str("bar.v.double "),
        "rVert" => output.push_str("bar.v.double "),

        // Big delimiters - handled via data module
        _ if crate::data::symbols::is_big_delimiter_command(base_name) => {
            if let Some(delim) = conv.get_required_arg(&cmd, 0) {
                if let Some(typst_delim) = crate::data::symbols::convert_delimiter(delim.trim()) {
                    if !typst_delim.is_empty() {
                        output.push_str(typst_delim);
                        output.push(' ');
                    }
                } else {
                    output.push_str(delim.trim());
                    output.push(' ');
                }
            }
        }

        // Custom Operators with limits
        "argmin" | "argmax" | "Argmin" | "Argmax" => {
            let op_name = match base_name {
                "Argmin" => "Argmin",
                "Argmax" => "Argmax",
                "argmax" => "argmax",
                _ => "argmin",
            };
            let _ = write!(output, "limits(op(\"{}\")) ", op_name);
        }

        // Custom Operators without limits
        "Var" | "Cov" | "Corr" | "tr" | "Tr" | "diag" | "rank" | "sgn" | "sign"
        | "supp" | "proj" | "prox" | "dist" | "dom" | "epi" | "graph" | "conv"
        | "softmax" | "relu" | "ReLU" | "KL" => {
            let op_name = match base_name {
                "tr" | "Tr" => "tr",
                "relu" | "ReLU" => "ReLU",
                _ => base_name,
            };
            let _ = write!(output, "op(\"{}\") ", op_name);
        }

        // Special symbols
        "E" => output.push_str("bb(E) "),
        "iid" => output.push_str("\"i.i.d.\""),

        // Negation command - \not followed by a symbol
        "not" => {
            // \not X -> X.not (for symbols that support it)
            // or cancel(X) as fallback
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                let arg = arg.trim();
                // Common negatable symbols
                let negated = match arg {
                    "=" | "eq" => "eq.not",
                    "<" | "lt" => "lt.not",
                    ">" | "gt" => "gt.not",
                    "in" => "in.not",
                    "subset" => "subset.not",
                    "supset" => "supset.not",
                    "equiv" => "equiv.not",
                    "approx" => "approx.not",
                    "sim" | "tilde.op" => "tilde.not",
                    "parallel" => "parallel.not",
                    "exists" => "exists.not",
                    "ni" | "in.rev" => "in.rev.not",
                    "mid" | "divides" => "divides.not",
                    "prec" => "prec.not",
                    "succ" => "succ.not",
                    "subset.eq" => "subset.eq.not",
                    "supset.eq" => "supset.eq.not",
                    "lt.eq" => "lt.eq.not",
                    "gt.eq" => "gt.eq.not",
                    "arrow.l" => "arrow.l.not",
                    "arrow.r" => "arrow.r.not",
                    "arrow.l.double" => "arrow.l.double.not",
                    "arrow.r.double" => "arrow.r.double.not",
                    "tack.r" => "tack.r.not",
                    "forces" => "forces.not",
                    _ => {
                        // Try appending .not for any symbol
                        if arg.chars().all(|c| c.is_alphanumeric() || c == '.') {
                            // Output as symbol.not
                            let _ = write!(output, "{}.not ", arg);
                            return;
                        } else {
                            // Fallback: use cancel
                            let _ = write!(output, "cancel({}) ", arg);
                            return;
                        }
                    }
                };
                output.push_str(negated);
                output.push(' ');
            }
        }

        "R" => output.push_str("R"),
        "Bibtex" => output.push_str("BibTeX"),
        "Biblatex" => output.push_str("BibLaTeX"),
        "verb" => {
            if let Some(content) = conv.get_required_arg(&cmd, 0) {
                write_inline_raw(output, content.trim(), None);
            } else {
                let text = cmd.syntax().text().to_string();
                for delim in ['|', '!', '+', '@', '#', '"', '\''] {
                    let pattern = format!("verb{}", delim);
                    if let Some(start) = text.find(&pattern) {
                        let rest = &text[start + pattern.len()..];
                        if let Some(end) = rest.find(delim) {
                            let code = &rest[..end];
                            write_inline_raw(output, code.trim(), None);
                            break;
                        }
                    }
                }
            }
        }
        "lstinline" => {
            if let Some(content) = conv.get_required_arg(&cmd, 0) {
                let options_str = conv.get_optional_arg(&cmd, 0).unwrap_or_default();
                let options = CodeBlockOptions::parse(&options_str);
                let lang = options.get_typst_language();
                let lang_opt = if lang.is_empty() { None } else { Some(lang) };
                write_inline_raw(output, content.trim(), lang_opt);
            }
        }

        // Algorithm/problem info macros (custom templates)
        "problemInfo" => {
            let title = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            if !title.trim().is_empty() {
                let _ = writeln!(output, "\n#strong[{}]\n", title.trim());
            }
        }
        "algoInfo" => {
            let title = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            if !title.trim().is_empty() {
                let _ = writeln!(output, "\n#strong[{}]\n", title.trim());
            }
        }
        "dataInfos" => {
            let label = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            let body = conv
                .convert_required_arg(&cmd, 1)
                .or_else(|| conv.get_required_arg(&cmd, 1))
                .unwrap_or_default();
            if !label.trim().is_empty() {
                let _ = write!(output, "*{}:* ", label.trim());
            }
            if !body.trim().is_empty() {
                output.push_str(body.trim());
            }
            output.push('\n');
        }
        "dataInfo" => {
            let label = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            let body = conv
                .convert_required_arg(&cmd, 1)
                .or_else(|| conv.get_required_arg(&cmd, 1))
                .unwrap_or_default();
            if !label.trim().is_empty() {
                let _ = write!(output, "*{}:* ", label.trim());
            }
            if !body.trim().is_empty() {
                output.push_str(body.trim());
            }
            output.push('\n');
        }
        "algoSteps" => {
            let body = conv
                .convert_required_arg(&cmd, 0)
                .or_else(|| conv.get_required_arg(&cmd, 0))
                .unwrap_or_default();
            if !body.trim().is_empty() {
                output.push_str(body.trim());
                output.push('\n');
            }
        }

        // Pseudocode / game macros (cryptocode-style)
        "pcln" => output.push_str("\\ "),
        "pcreturn" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "return {} ", content.trim());
            } else {
                output.push_str("return ");
            }
        }
        "pccomment" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "/* {} */ ", content.trim());
            }
        }
        "gamechange" | "gameprocedure" | "gameproof" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(content.trim());
                output.push(' ');
            } else {
                output.push_str(base_name);
                output.push(' ');
            }
        }
        "pcfor" => {
            output.push_str("for ");
        }
        "leqP" => {
            output.push_str("<=_P");
        }
        "subsetneq" => {
            output.push('⊊');
        }
        "kk" => {
            output.push('k');
        }
        "p" => {
            output.push('p');
        }
        "pnm" => {
            output.push_str("pnm");
        }
        "toG" => {
            output.push_str("toG");
        }
        "t" => {
            if let Some(arg) = conv.convert_required_arg(&cmd, 0) {
                output.push_str(arg.trim());
            } else {
                output.push('t');
            }
        }
        "ipcf" | "scp" | "erz" | "SSCCA" | "id" | "prompt" | "child" | "layers"
        | "Def" | "Ex" | "Rq" | "q" => {
            output.push_str(base_name);
            output.push(' ');
        }
        "mintinline" => {
            let lang_raw = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let content = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            let lang = LANGUAGE_MAP.get(lang_raw.as_str()).copied().unwrap_or("");
            let lang_opt = if lang.is_empty() { None } else { Some(lang) };
            write_inline_raw(output, content.trim(), lang_opt);
        }

        // QED symbols
        "qed" | "qedsymbol" | "qedhere" => output.push('∎'),

        "fig" => {
            if let Some(content) = conv.convert_required_arg(&cmd, 0) {
                let _ = write!(output, "Fig. {}", content.trim());
            } else {
                output.push_str("Fig.");
            }
        }

        // Special character commands (Scandinavian, etc.)
        "o" => output.push('ø'),   // \o -> ø
        "O" => output.push('Ø'),   // \O -> Ø
        "aa" => output.push('å'),  // \aa -> å
        "AA" => output.push('Å'),  // \AA -> Å
        "ae" => output.push('æ'),  // \ae -> æ
        "AE" => output.push('Æ'),  // \AE -> Æ
        "oe" => output.push('œ'),  // \oe -> œ
        "OE" => output.push('Œ'),  // \OE -> Œ
        "ss" => output.push('ß'),  // \ss -> ß
        "at" => output.push('@'),  // \at -> @
        "nobreakdash" => output.push('-'),
        "<" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push_str("angle.l ");
            } else {
                output.push('<');
            }
        }
        ">" => {
            if matches!(conv.state.mode, ConversionMode::Math) {
                output.push_str("angle.r ");
            } else {
                output.push('>');
            }
        }

        // Newcommand and def in body
        "newcommand" | "renewcommand" | "providecommand" => {
            handle_newcommand(conv, &cmd);
        }
        "def" => {
            handle_def(conv, &cmd);
        }
        "DeclareMathOperator" | "DeclareMathOperator*" => {
            handle_declare_math_operator(conv, &cmd, base_name.ends_with('*'));
        }

        // Page breaks
        "newpage" | "pagebreak" => {
            let is_two_column = conv
                .state
                .document_class_info
                .as_ref()
                .map(|info| info.columns > 1)
                .unwrap_or(false)
                || matches!(
                    conv.state.template_kind,
                    Some(
                        super::context::TemplateKind::Cvpr
                            | super::context::TemplateKind::Iclr
                            | super::context::TemplateKind::Icml
                            | super::context::TemplateKind::Neurips
                    )
                );
            if is_two_column {
                output.push_str("\n#colbreak()\n");
            } else {
                output.push_str("\n#pagebreak()\n");
            }
        }

        // Appendix
        "appendix" => {
            output.push_str("\n// Appendix\n");
            output.push_str("#counter(heading).update(0)\n");
            output.push_str("#set heading(numbering: \"A.\")\n\n");
        }

        // Hyperref string alternate - prefer TeX string
        "texorpdfstring" => {
            let tex = conv.convert_required_arg(&cmd, 0).unwrap_or_default();
            if !tex.trim().is_empty() {
                output.push_str(tex.trim());
                output.push(' ');
            }
        }
        "linkbutton" => {
            let url = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let label = conv.get_required_arg(&cmd, 2).unwrap_or_default();
            let label = if label.trim().is_empty() {
                url.clone()
            } else {
                super::utils::convert_caption_text(&label)
            };
            if !url.trim().is_empty() {
                let escaped = super::utils::escape_typst_text(url.trim());
                let _ = write!(output, "#link(\"{}\")[{}] ", escaped, label.trim());
            } else if !label.trim().is_empty() {
                output.push_str(label.trim());
                output.push(' ');
            }
        }

        // Color definitions inside body: record and ignore output
        "definecolor" => {
            let name = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let model = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            let spec = conv.get_required_arg(&cmd, 2).unwrap_or_default();
            if !name.trim().is_empty() && !model.trim().is_empty() && !spec.trim().is_empty() {
                let ident = sanitize_color_identifier(name.trim());
                let value = parse_color_with_model(model.trim(), spec.trim());
                conv.state.register_color_def(ident, value);
            }
        }
        "colorlet" => {
            let name = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let spec = conv.get_required_arg(&cmd, 1).unwrap_or_default();
            if !name.trim().is_empty() && !spec.trim().is_empty() {
                let ident = sanitize_color_identifier(name.trim());
                let value = sanitize_color_expression(spec.trim());
                conv.state.register_color_def(ident, value);
            }
        }

        // Color command (scope-based, hard to convert perfectly)
        "color" => {
            // \color{red} affects following text until scope ends
            // Typst doesn't have an equivalent - output as comment with mapped color
            let color_name = conv.get_required_arg(&cmd, 0).unwrap_or_default();
            let typst_color = resolve_color_expression(conv, &color_name);
            let _ = write!(output, "/* \\color{{{}}} -> {} */", color_name, typst_color);
        }

        // Ignored commands - alignment and layout
        "centering" | "raggedright" | "raggedleft" | "noindent" | "indent"
        | "nopagebreak" | "enlargethispage"
        | "null" | "relax" | "ignorespaces" | "obeylines" | "obeyspaces" | "frenchspacing"
        | "nonfrenchspacing" | "normalfont" | "rmfamily" | "sffamily" | "ttfamily" | "bfseries"
        | "mdseries" | "itshape" | "scshape" | "upshape" | "slshape" | "normalsize" | "tiny"
        | "scriptsize" | "footnotesize" | "small" | "large" | "Large" | "LARGE" | "huge"
        | "Huge" | "nocite" | "printbibliography" | "printglossary" | "printglossaries" | "printacronyms"
        | "glsresetall" | "listoffigures" | "listoftables" | "addtocontents" | "addtotoc"
        | "contentsline" | "numberline" | "addContents"
        | "frontmatter" | "mainmatter" | "backmatter"
        // IEEE and conference specific
        | "IEEEauthorblockN" | "IEEEauthorblockA" | "IEEEoverridecommandlockouts"
        | "IEEEaftertitletext" | "IEEEmembership" | "IEEEspecialpapernotice"
        | "markboth" | "markright" | "thanks" | "and"
        | "authorrunning" | "titlerunning" | "institute"
        | "icmltitlerunning" | "icmlsetsymbol" | "printAffiliationsAndNotice" | "icmlEqualContribution"
        | "iclrfinalcopy"
        | "address" | "subjclass" | "subclass" | "ccsdesc" | "received" | "PACS" | "altaffiliation"
        | "CCSXML" | "authornote"
        | "acmArticle" | "acmDOI" | "acmJournal" | "acmMonth" | "acmNumber" | "acmVolume"
        | "acmYear" | "acmConference" | "acmBooktitle" | "acmPrice" | "acmISBN"
        | "copyrightyear"
        // Thesis template setup
        | "DocumentMetadata" | "IfPackageAtLeastTF" | "newcolumntype" | "AtEveryBibitem"
        | "renewbibmacro" | "DeclarePairedDelimiterXPP" | "pdfbookmark"
        | "jot" | "ifpdftex" | "else" | "fi" | "setstretch" | "dnormalspacing"
        | "hbadness" | "hyphenation" | "textwidth" | "textheight" | "footskip" | "headsep" | "topmargin" | "setcounter"
        | "setlength" | "addtolength" | "parindent" | "baselineskip" | "addvspace"
        | "hruleheight" | "abovedisplayshortskip"
        | "newgeometry" | "restoregeometry"
        | "beforepreface" | "afterpreface" | "onlinesignature"
        | "lstset" | "lstdefinestyle" | "chaptermark" | "MakeOuterQuote" | "tolerance"
        | "copyrightpage" | "pagestyle" | "thispagestyle"
        | "fancyhead" | "fancyfoot" | "fancyhf" | "fancyheadings" | "fancypagestyle"
        | "lhead" | "chead" | "rhead" | "lfoot" | "cfoot" | "rfoot"
        // Additional formatting switches (excluding already handled: it, bf, tt, sc, rm)
        | "em" | "sf" | "sl" | "justifying" | "justify"
        // Floats and placement
        | "suppressfloats" | "FloatBarrier" | "clearfloats"
        // Spacing (excluding already handled: smallskip, medskip, bigskip)
        | "vfill" | "hfill" | "hfil" | "vfil" | "break" | "allowbreak" | "nobreak"
        | "goodbreak" | "penalty" | "kern" | "hss" | "looseness" | "xspace"
        | "interlinepenalty" | "midsloppy" | "raggedbottom" | "doublespace"
        // Margin and page setup
        | "marginpar" | "marginparpush" | "reversemarginpar" | "normalmarginpar"
        // Misc invisible commands (excluding already handled: protect)
        | "expandafter" | "global" | "long" | "outer" | "inner"
        | "noexpand" | "csname" | "endcsname" | "string" | "number" 
        // More bibliography
        | "bibstyle" | "bibliographystyle" | "defbibheading" | "bibitemsep" | "makebib"
        // Index
        | "makeindex" | "printindex" | "index" | "glossary"
        // Equation numbering control
        | "nonumber" | "notag" | "balance" | "normalcolor" | "setbox" | "wd"
        | "newblock" | "boldmath" | "unboldmath"
        // Internal/class helpers
        | "@ifstar" | "@startsection" | "z@" | "elvbf" | "gaussianlbmain"
        // Math style switches
        | "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle"
        | "abovedisplayskip" | "belowdisplayskip"
        | "linewidth" | "noalign" | "arraybackslash" | "graphicspath"
        | "arrayrulewidth" | "extrarowheight" | "rowcolor" | "rowcolors"
        | "itemsep" | "addtocounter" | "addlinespace" | "cdashline" | "compactitem"
        | "onecolumn" | "pacs" | "orcid" | "let" | "@dashdrawstore" | "adl@draw"
        | "adl@drawiv" | "parskip" | "tabcolsep" | "restylealgo" | "setcopyright"
        | "theequation" | "fontsize" | "selectfont" | "@footnotetext" | "subfile"
        | "acronymtype" | "makecover" | "includepdf" | "spacing"
        | "leftmark" | "rightmark" | "lstlistoflistings" | "maxtocdepth"
        | "onehalfspace" | "singlespace" | "subtitle" | "signaturepage"
        | "captionsetup" | "maketitlesupplementary" | "titleformat" | "titlespacing"
        | "makeatletter" | "makeatother" | "ifcase" | "or" | "month" | "year" | "space"
        | "the" | "ttfamilyError" | "mathindent" | "mskip" | "ifx" | "alph" | "monthyear"
        | "." | "r" | "PackageError" | "object" | "leftmargin" | "parsep"
        | "begingroup" | "endgroup" | "gdef" | "@thefnmark"
        | "bgroup" | "egroup" | "endinput" | "futurelet"
        | "@let@token" | "@onedot" | "@arstdepth" | "@otarlinesep" | "@unrecurse"
        | "eads" | "submitto" | "numparts" | "endnumparts" | "thetable"
        | "beginsupplement" | "ack" | "corref" | "mailsa" | "toctitle" | "tocauthor"
        | "IEEEbiographynophoto" | "adjustlimits"
        | "extracolsep" | "EndOfBibitem" | "bibitem" | "bibentry" | "endhead"
        | "mciteBstWouldAddEndPuncttrue" | "mciteSetBstMidEndSepPunct"
        | "mcitedefaultmidpunct" | "mcitedefaultendpunct" | "mcitedefaultseppunct"
        | "mciteundefinedmacro" | "mcitethebibliography"
        | "mciteSetBstSublistMode" | "mciteSetBstMaxWidthForm"
        | "mciteSetBstSublistLabelBeginEnd"
        | "ifCLASSOPTIONcompsoc" | "ifCLASSOPTIONcaptionsoff"
        | "mathpalette" | "mkern" | "sloppy" | "ignorespacesrelation"
        | "refstepcounter" | "theoremstyle" | "cellspacetoplimit" | "cellspacebottomlimit"
        | "widebarargheight" | "widebarargwidth" | "widebarargdepth"
        | "settoheight" | "settodepth" | "settowidth"
        | "phantomsection" | "minitoc" | "flushbottom" | "cochairs" | "cochair" | "import" | "usepackage"
        | "endminipage" | "ifABpages" | "citing"
        | "HRule" | "auxsettings" | "c@page" | "chapterstyle" | "chapwithtoc"
        | "cleardoubleemptypage" | "counterwithout" | "filright"
        | "hfillPage" | "if@twocolumn" | "if@twoside" | "ifodd" | "joint" | "originally"
        | "line" | "makefrontcover" | "parI" | "renewcaptionname" | "@mkboth" | "@starttoc" | "AtBeginEnvironment"
        | "LargeLATVIJAS" | "Metadata" | "NKTsetup" | "OnehalfSpacing" | "RaggedRight"
        | "RomanNumbering" | "arabicNumbering" | "alignOddPage"
        | "THEDAY" | "THEMONTH" | "THEYEAR"
        | "announcement" | "apptocmd" | "authorshipDeclaration"
        | "microtypesetup" | "nomenclature" | "putbib" | "textbaselineskip"
        | "DTMenglishmonthname" | "DTMenglishordinal" | "ExplSyntaxOn" | "ExplSyntaxOff"
        | "SingleSpacing" | "bibsep"
        | "dominitoc" | "droptitle" | "frontmatterbaselineskip" | "leavevmode"
        | "makeacknowledgement" | "makebibliography" | "makecopyright" | "makededication"
        | "mtcaddchapter" | "oneappendix" | "printthesisindex"
        | "oldnumberline"
        | "restorepagenumber" | "savepagenumber" | "NoBgThispage" | "beginL" | "endL"
        | "scriptsizessp" | "smallbreak" | "smallssp" | "startappendices"
        | "tagpdfsetup" | "theendnotes"
        | "appendixpage" | "cftbeforechapskip" | "cftchapnumwidth" | "coverimage" | "zihao"
        | "body" | "hypersetup" | "topskip" | "titlepage" | "thesis"
        | "ht" | "hei" | "T" | "Urlmuskip" | "strutbox" | "ifpdf" | "version"
        | "per" | "nouppercase" | "hangindent" | "origaddvspace" | "chaptertitlefont"
        | "oddsidemargin" | "evensidemargin" | "headheight" | "linenumbers"
        | "cftpagenumbersoff" | "cftpagenumberson" | "cftsecnumwidth" | "cftsubsecnumwidth"
        | "cftsubsecindent" | "cftfignumwidth" | "cftfigindent" | "cfttabnumwidth"
        | "cfttabindent" | "cftbeforetabskip" | "cftbeforetoctitleskip" | "cftaftertoctitleskip"
        | "shusetup" | "zhdigits" | "zhnumber" | "shorthandoff"
        | "section@cntformat" | "subsection@cntformat" | "ps@plain" | "ps@main"
        | "enableindents" | "sourceatright" | "center" | "changefont"
        | "bookmarksetup" | "titlecontents" | "thecontentslabel" | "titlerule"
        | "setglossarystyle" | "glossarystyle" | "glsaddall"
        | "covercredit" | "song" | "songti" | "btypeout"
        | "aliaspagestyle" | "clearforchapter"
        | "addvspacetoc" | "addappheadtotoc"
        | "maketitlepage" | "titlepageMS" | "signaturepageMS" | "makecoverpage"
        | "makeapproval" | "makedeclaration" | "frontpage" | "makefirstpage"
        | "interfootnotelinepenalty" | "emergencystretch" | "linepenalty" | "hyphenpenalty"
        | "resetpagenumbering" | "setlanguage" | "selectlanguage"
        | "newlist" | "setlist" | "storeinipagenumber"
        | "definecdlabeloffsets" | "createcdlabel" | "createcdcover"
        | "makefrontmatter" | "beginfrontmatter" | "beginmainmatter"
        | "includeabbreviations"
        | "@seccntformat" | "@ifundefined"
        | "mdtheorem" | "AddToShipoutPictureBG" | "AtNextBibliography" | "arial"
        | "chaptertitlename" | "linespread" | "mdfdefinestyle" | "noappendicestocpagenum"
        | "place" | "thechapter" | "thesection" | "thesubsection" | "uselogo"
        | "declaretypist" | "@dtm@day" | "@dtm@month" | "@dtm@year"
        | "certificatewidth" | "hboxto" | "vrulewidth"
        | "ifdefined" | "ifvmode" | "if" | "aaaianonymous" | "ECCVyear" | "ECCVyearSubmission"
        | "mathsfit" | "emrequire" | "emet" | "eme" | "par@fig" | "links"
        | "advance" | "ifnum" | "iftoggle" | "contents" | "references" | "postfacesection"
        | "degreeC" | "@twosidetrue" | "@ifnextchar" | "@height" | "@skip@bove"
        | "@yhline" | "@zhline" | "csnamecrcr" | "extrarulesep"
        | "epigraphhead" | "nobibliography" | "normallinespacing" => {
            // Ignore these
        }

        // Try symbol maps for unknown commands
        _ => {
            if matches!(conv.state.mode, ConversionMode::Text) {
                let table_rules = [
                    "toprule",
                    "midrule",
                    "bottomrule",
                    "hline",
                    "hhline",
                    "cline",
                    "cmidrule",
                ];
                for rule in table_rules {
                    if let Some(rest) = base_name.strip_prefix(rule) {
                        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                            output.push_str("|||HLINE|||");
                            output.push(' ');
                            super::utils::escape_typst_text_into(rest, output);
                            output.push(' ');
                            return;
                        }
                    }
                }
                if let Some(rest) = base_name.strip_prefix("noindent") {
                    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphabetic()) {
                        super::utils::escape_typst_text_into(rest, output);
                        output.push(' ');
                        return;
                    }
                }
                if let Some(rest) = base_name.strip_prefix("item") {
                    if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphabetic()) {
                        output.push('\n');
                        for _ in 0..conv.state.indent {
                            output.push(' ');
                        }
                        match conv.state.current_env() {
                            EnvironmentContext::Enumerate => output.push_str("+ "),
                            EnvironmentContext::Description | EnvironmentContext::Itemize => {
                                output.push_str("- ")
                            }
                            _ => {}
                        }
                        super::utils::escape_typst_text_into(rest, output);
                        output.push(' ');
                        return;
                    }
                }
                if let Some(rest) = base_name.strip_prefix("and") {
                    if !rest.is_empty() && rest.chars().next().unwrap_or('a').is_ascii_uppercase()
                    {
                        super::utils::escape_typst_text_into(rest, output);
                        output.push(' ');
                        return;
                    }
                }
                if let Some(rest) = base_name.strip_prefix("normalsize") {
                    if !rest.is_empty() && rest.chars().next().unwrap_or('a').is_ascii_uppercase()
                    {
                        super::utils::escape_typst_text_into(rest, output);
                        output.push(' ');
                        return;
                    }
                }
                if let Some(rest) = base_name.strip_prefix("it") {
                    let first = rest.chars().next().unwrap_or('a');
                    if rest.len() == 1 || first.is_ascii_uppercase() {
                        output.push_str("#emph[");
                        super::utils::escape_typst_text_into(rest, output);
                        output.push_str("] ");
                        return;
                    }
                }
                if let Some(rest) = base_name.strip_prefix("tt") {
                    let first = rest.chars().next().unwrap_or('a');
                    if rest.len() <= 2 || first.is_ascii_uppercase() {
                        output.push('`');
                        super::utils::escape_typst_text_into(rest, output);
                        output.push('`');
                        output.push(' ');
                        return;
                    }
                }
            }
            if let Some(rest) = base_name.strip_prefix("par") {
                if !rest.is_empty() && rest.chars().next().unwrap_or('a').is_ascii_uppercase() {
                    output.push_str("\n\n");
                    output.push_str(rest);
                    output.push(' ');
                    return;
                }
            }
            if let Some(rest) = base_name.strip_prefix("or") {
                if !rest.is_empty() && rest.chars().next().unwrap_or('a').is_ascii_uppercase() {
                    output.push_str(rest);
                    output.push(' ');
                    return;
                }
            }
            if let Some(rest) = base_name.strip_prefix("normalsize") {
                if !rest.is_empty() && rest.chars().next().unwrap_or('a').is_ascii_uppercase() {
                    super::utils::escape_typst_text_into(rest, output);
                    output.push(' ');
                    return;
                }
            }
            if let Some(rest) = base_name.strip_prefix("textbackslash") {
                output.push('\\');
                if !rest.is_empty() {
                    super::utils::escape_typst_text_into(rest, output);
                }
                output.push(' ');
                return;
            }
            let lookup = format!("\\{}", base_name);
            if let Some(val) = crate::siunitx::SI_UNITS.get(lookup.as_str()) {
                output.push_str(val);
                output.push(' ');
                return;
            }
            if let Some(val) = crate::siunitx::SI_PREFIXES.get(lookup.as_str()) {
                output.push_str(val);
                output.push(' ');
                return;
            }

            // Try symbol lookup
            if let Some(typst) = lookup_symbol(base_name) {
                // In math mode, ensure space before alphabetic symbols if previous char was digit
                if matches!(conv.state.mode, ConversionMode::Math)
                    && typst.starts_with(|c: char| c.is_ascii_alphabetic())
                    && !output.is_empty()
                    && !output.ends_with(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == '{')
                {
                    output.push(' ');
                }
                output.push_str(typst);
                output.push(' ');
                return;
            }

            // Check text format commands (these return prefix/suffix pairs)
            let lookup_name = format!("\\{}", base_name);
            if let Some((prefix, suffix)) = TEXT_FORMAT_COMMANDS.get(lookup_name.as_str()) {
                if let Some(content) = conv.get_required_arg(&cmd, 0) {
                    output.push_str(prefix);
                    output.push_str(&content);
                    output.push_str(suffix);
                }
                return;
            }

            if matches!(conv.state.mode, ConversionMode::Math) {
                let prefixes = [
                    "times",
                    "in",
                    "geq",
                    "leq",
                    "ge",
                    "le",
                    "propto",
                    "coloneqq",
                    "triangleq",
                    "cap",
                    "bigcup",
                    "backslash",
                    "rightarrow",
                ];
                for prefix in prefixes {
                    if let Some(rest) = base_name.strip_prefix(prefix) {
                        let rest_is_word = rest.chars().all(|c| c.is_ascii_alphanumeric());
                        let allow_short_rest = rest.len() == 1
                            || ((prefix == "leq" || prefix == "geq") && rest.len() <= 2);
                        if rest_is_word && allow_short_rest {
                            // Add leading space before alphabetic symbols if needed
                            if !output.is_empty()
                                && !output.ends_with(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == '{')
                            {
                                output.push(' ');
                            }
                            if let Some(typst) = lookup_symbol(prefix) {
                                output.push_str(typst);
                            } else {
                                output.push_str(prefix);
                            }
                            output.push(' ');
                            output.push_str(rest);
                            output.push(' ');
                            return;
                        }
                    }
                }

                let bracket_prefixes = [
                    ("langle", "angle.l"),
                    ("rangle", "angle.r"),
                    ("lvert", "bar.v"),
                    ("rvert", "bar.v"),
                ];
                for (prefix, sym) in bracket_prefixes {
                    if let Some(rest) = base_name.strip_prefix(prefix) {
                        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                            output.push_str(sym);
                            output.push(' ');
                            output.push_str(rest);
                            output.push(' ');
                            return;
                        }
                    }
                }
            }

            if try_expand_shorthand_command(conv, base_name, output) {
                return;
            }

            if try_split_merged_command(conv, base_name, output) {
                return;
            }

            // Handle LaTeX internal commands that start with @
            // \@ is a spacing control, \@ssec etc. should just output the part after @
            if let Some(rest) = base_name.strip_prefix('@') {
                if rest.is_empty() {
                    // \@ alone - just strip it (spacing control)
                    return;
                }
                // Output the rest without the @ (this handles \@ssec -> ssec, etc.)
                output.push_str(rest);
                output.push(' ');
                return;
            }

            let context = match conv.state.mode {
                ConversionMode::Math => Some("math".to_string()),
                ConversionMode::Text => Some("text".to_string()),
            };
            let loss_id = conv.record_loss(
                LossKind::UnknownCommand,
                Some(base_name.to_string()),
                format!("Unknown command \\{}", base_name),
                Some(cmd.syntax().text().to_string()),
                context,
            );
            let loss_marker = format!(" /* {}{} */ ", LOSS_MARKER_PREFIX, loss_id);

            // Pass through unknown commands using AST-based processing
            // This preserves the behavior of convert_default_command from old version
            if conv.state.options.non_strict {
                use mitex_parser::syntax::SyntaxKind;

                if matches!(conv.state.mode, ConversionMode::Math) {
                    output.push_str(&loss_marker);
                    // In math mode, unknown commands often represent styling macros.
                    // Prefer emitting just the arguments (if any) to avoid undefined functions.
                    let mut has_args = false;
                    let mut first = true;
                    for child in cmd.syntax().children_with_tokens() {
                        if child.kind() == SyntaxKind::ClauseArgument {
                            if !first {
                                output.push(' ');
                            }
                            first = false;
                            has_args = true;
                            if let SyntaxElement::Node(n) = child {
                                conv.visit_node(&n, output);
                            }
                        }
                    }
                    // If no arguments, don't emit anything extra in math mode.
                    // The loss marker already records this was a loss.
                } else {
                    // In text mode, output name as comment to avoid garbage text
                    let _ = write!(output, "{} /* \\{} */", loss_marker, base_name);
                    for child in cmd.syntax().children_with_tokens() {
                        if child.kind() == SyntaxKind::ClauseArgument {
                            if let SyntaxElement::Node(n) = child {
                                conv.visit_node(&n, output);
                            }
                        }
                    }
                }
            } else {
                conv.state.warnings.push(format!("Unknown command: {}", cmd_str));
            }
        }
    }
}

fn try_expand_shorthand_command(
    conv: &mut LatexConverter,
    base_name: &str,
    output: &mut String,
) -> bool {
    match conv.state.mode {
        ConversionMode::Math => {
            if let Some(rest) = base_name.strip_prefix("mathbf") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "upright(bold({})) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("bf") {
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphabetic()) {
                    let _ = write!(output, "bold({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("mathcal") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "cal({}) ", rest);
                    return true;
                }
            }
            for prefix in ["mathbb", "mathds", "mathbbm"] {
                if let Some(rest) = base_name.strip_prefix(prefix) {
                    if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                        if matches!(rest, "R" | "Z" | "N" | "C" | "Q") {
                            let _ = write!(output, "{}{} ", rest, rest);
                        } else {
                            let _ = write!(output, "bb({}) ", rest);
                        }
                        return true;
                    }
                }
            }
            if let Some(rest) = base_name.strip_prefix("mathfrak") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "frak({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("mathscr") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "scr({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("mathsf") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "sans({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("mathtt") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "mono({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("bm") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "bold({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("bar") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "overline({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("over") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "overline({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("rm") {
                if rest == "argmin" || rest == "argmax" || rest == "diag" {
                    let _ = write!(output, "op(\"{}\") ", rest);
                    return true;
                }
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    let _ = write!(output, "upright({}) ", rest);
                    return true;
                }
            }
            if let Some(rest) = base_name.strip_prefix("forall") {
                if rest.len() == 1 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                    output.push_str("forall ");
                    output.push_str(rest);
                    output.push(' ');
                    return true;
                }
            }
        }
        ConversionMode::Text => {
            if let Some(rest) = base_name.strip_prefix("bf") {
                if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphabetic()) {
                    let _ = write!(output, "*{}* ", rest);
                    return true;
                }
            }
        }
    }

    false
}

fn try_split_merged_command(
    conv: &mut LatexConverter,
    base_name: &str,
    output: &mut String,
) -> bool {
    if base_name.len() < 2 {
        return false;
    }

    let is_math = matches!(conv.state.mode, ConversionMode::Math);

    for i in (2..base_name.len()).rev() {
        let prefix = &base_name[..i];
        let suffix = &base_name[i..];
        if !suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue;
        }

        if let Some(macro_def) = conv.state.macros.get(prefix).cloned() {
            if let Some(expanded) = expand_zero_arg_macro(conv, &macro_def) {
                output.push_str(expanded.trim_end());
                output.push(' ');
                output.push_str(suffix);
                output.push(' ');
                return true;
            }
        }

        if is_math {
            if let Some(sym) = lookup_symbol(prefix) {
                // Add leading space before alphabetic symbols if needed
                if sym.starts_with(|c: char| c.is_ascii_alphabetic())
                    && !output.is_empty()
                    && !output.ends_with(|c: char| c.is_whitespace() || c == '(' || c == '[' || c == '{')
                {
                    output.push(' ');
                }
                output.push_str(sym);
                output.push(' ');
                output.push_str(suffix);
                output.push(' ');
                return true;
            }
        }

        if let Some(spacing) = match prefix {
            "quad" => Some("  "),
            "qquad" => Some("    "),
            "enspace" => Some(" "),
            "thinspace" => Some(" "),
            "thickspace" => Some("  "),
            _ => None,
        } {
            output.push_str(spacing);
            output.push_str(suffix);
            output.push(' ');
            return true;
        }
    }

    false
}

fn expand_zero_arg_macro(conv: &mut LatexConverter, macro_def: &MacroDef) -> Option<String> {
    if macro_def.num_args != 0 {
        return None;
    }
    if let Some(cached) = conv.state.macro_cache.get(&macro_def.name) {
        return Some(cached.clone());
    }
    let mut output = String::new();
    let tree = mitex_parser::parse(&macro_def.replacement, conv.spec.clone());
    conv.visit_node(&tree, &mut output);
    conv.state
        .macro_cache
        .insert(macro_def.name.clone(), output.clone());
    Some(output)
}

fn resolve_color_expression(conv: &mut LatexConverter, raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "black".to_string();
    }

    if is_named_color(trimmed) {
        return sanitize_color_expression(trimmed);
    }

    let is_ident = trimmed.chars().enumerate().all(|(i, ch)| {
        (i > 0 || !ch.is_ascii_digit()) && (ch.is_ascii_alphanumeric() || ch == '_')
    });
    if is_ident {
        let ident = sanitize_color_identifier(trimmed);
        if !conv.state.color_defs.iter().any(|(name, _)| name == &ident) {
            conv.state
                .register_color_def(ident.clone(), "black".to_string());
        }
        return ident;
    }

    sanitize_color_expression(trimmed)
}

// =============================================================================
// Helper functions
// =============================================================================

/// Handle \newcommand or \renewcommand
fn handle_newcommand(conv: &mut LatexConverter, cmd: &CmdItem) {
    // \newcommand{\name}[nargs][default]{replacement}
    let name = conv
        .get_required_arg(cmd, 0)
        .map(|n| n.trim_start_matches('\\').to_string());
    let replacement = conv.get_required_arg(cmd, 1);

    if let (Some(name), Some(replacement)) = (name, replacement) {
        let num_args = conv
            .get_optional_arg(cmd, 0)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let default_arg = conv.get_optional_arg(cmd, 1);

        conv.state.macros.insert(
            name.clone(),
            MacroDef {
                name,
                num_args,
                default_arg,
                replacement,
            },
        );
    }
}

/// Handle \def
fn handle_def(conv: &mut LatexConverter, cmd: &CmdItem) {
    // Parse \def\name{replacement} or \def\name#1#2{replacement}
    // The syntax is: \def<control-sequence><parameter-text>{<replacement>}

    // Extract the raw text of the entire \def command (with braces preserved)
    let full_text = super::utils::extract_node_text_with_braces(cmd.syntax());

    // Pattern: starts with the macro name (e.g., \Loss, \R)
    // then optionally parameters (#1, #2, etc.), then {replacement}
    let text = full_text.trim();

    // Find the macro name - it should start with \
    if let Some(name_start) = text.find('\\') {
        let after_name = &text[name_start + 1..];
        // Find end of macro name (first non-alpha character)
        let name_end = after_name
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(after_name.len());
        let macro_name = &after_name[..name_end];

        if macro_name.is_empty() {
            return;
        }

        // Count parameter markers (#1, #2, etc.)
        let rest = &after_name[name_end..];
        let num_args = rest.matches('#').count().min(9);

        // Find the replacement text in braces - handle nested braces correctly
        if let Some(brace_start) = rest.find('{') {
            let after_brace = &rest[brace_start + 1..];
            // Find matching closing brace
            let mut depth = 1;
            let mut end_pos = 0;
            for (i, c) in after_brace.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end_pos = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let replacement = &after_brace[..end_pos];

            conv.state.macros.insert(
                macro_name.to_string(),
                MacroDef {
                    name: macro_name.to_string(),
                    num_args,
                    default_arg: None,
                    replacement: replacement.to_string(),
                },
            );
        }
    }
}

/// Handle \DeclareMathOperator and \DeclareMathOperator*
fn handle_declare_math_operator(conv: &mut LatexConverter, cmd: &CmdItem, is_star: bool) {
    let name = conv
        .get_required_arg(cmd, 0)
        .map(|n| n.trim().trim_start_matches('\\').to_string());
    let op_name = conv.get_required_arg(cmd, 1);

    if let (Some(name), Some(op_name)) = (name, op_name) {
        let op_name = op_name.trim();
        if op_name.is_empty() {
            return;
        }
        let replacement = if is_star {
            format!("\\operatorname*{{{}}}", op_name)
        } else {
            format!("\\operatorname{{{}}}", op_name)
        };
        conv.state.macros.insert(
            name.clone(),
            MacroDef {
                name,
                num_args: 0,
                default_arg: None,
                replacement,
            },
        );
    }
}

/// Handle \DeclarePairedDelimiter{\name}{\left}{\right}
fn handle_declare_paired_delimiter(conv: &mut LatexConverter, cmd: &CmdItem) {
    let name = conv
        .get_required_arg(cmd, 0)
        .map(|n| n.trim_start_matches('\\').to_string())
        .or_else(|| extract_paired_delimiter_name(cmd));
    let left = conv.get_required_arg(cmd, 1).unwrap_or_default();
    let right = conv.get_required_arg(cmd, 2).unwrap_or_default();

    if let Some(name) = name {
        let replacement = format!("\\left{} #1 \\right{}", left.trim(), right.trim());
        conv.state.macros.insert(
            name.clone(),
            MacroDef {
                name,
                num_args: 1,
                default_arg: None,
                replacement,
            },
        );
    }
}

/// Handle \newacronym
fn handle_newacronym(conv: &mut LatexConverter, cmd: &CmdItem) {
    let key = conv.get_required_arg(cmd, 0);
    let short = conv.get_required_arg(cmd, 1);
    let long = conv.get_required_arg(cmd, 2);

    if let (Some(key), Some(short), Some(long)) = (key, short, long) {
        conv.state.register_acronym(&key, &short, &long);
    }
}

/// Handle \newglossaryentry
fn handle_newglossaryentry(conv: &mut LatexConverter, cmd: &CmdItem) {
    let key = conv.get_required_arg(cmd, 0);
    let opts = conv.get_required_arg(cmd, 1).unwrap_or_default();

    if let Some(key) = key {
        // Parse name and description from opts
        let mut name = String::new();
        let mut description = String::new();

        for part in opts.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("name=") {
                name = n.trim_matches(|c| c == '{' || c == '}').to_string();
            } else if let Some(d) = part.strip_prefix("description=") {
                description = d.trim_matches(|c| c == '{' || c == '}').to_string();
            }
        }

        conv.state.register_glossary(&key, &name, &description);
    }
}

fn extract_item_label_fallback(cmd: &CmdItem) -> Option<String> {
    let text = cmd.syntax().text().to_string();
    let item_pos = text.find("\\item")?;
    let mut i = item_pos + "\\item".len();
    let bytes = text.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'[' {
        return None;
    }
    i += 1;
    let start = i;
    let mut depth = 1i32;
    while i < bytes.len() && depth > 0 {
        if bytes[i] == b'[' {
            depth += 1;
        } else if bytes[i] == b']' {
            depth -= 1;
        }
        i += 1;
    }
    if depth != 0 || i <= start {
        return None;
    }
    let end = i - 1;
    Some(text[start..end].trim().to_string())
}

fn extract_paired_delimiter_name(cmd: &CmdItem) -> Option<String> {
    let text = cmd.syntax().text().to_string();
    let marker = "\\DeclarePairedDelimiter";
    let pos = text.find(marker)?;
    let mut i = pos + marker.len();
    let bytes = text.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'\\' {
        return None;
    }
    i += 1;
    let start = i;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'@' || bytes[i] == b'_')
    {
        i += 1;
    }
    if i <= start {
        None
    } else {
        Some(text[start..i].to_string())
    }
}

/// Expand a user-defined macro
fn expand_user_macro(conv: &mut LatexConverter, cmd: &CmdItem, macro_def: &MacroDef) -> String {
    if macro_def.num_args == 0 {
        if let Some(cached) = conv.state.macro_cache.get(&macro_def.name) {
            return cached.clone();
        }
    }
    let mut result = macro_def.replacement.clone();

    // Collect arguments
    for i in 0..macro_def.num_args {
        let arg = conv
            .convert_required_arg(cmd, i)
            .or_else(|| macro_def.default_arg.clone())
            .unwrap_or_default();

        let placeholder = format!("#{}", i + 1);
        result = result.replace(&placeholder, &arg);
    }

    // Convert the expanded macro
    let mut output = String::new();
    let tree = mitex_parser::parse(&result, conv.spec.clone());
    conv.visit_node(&tree, &mut output);
    if macro_def.num_args == 0 {
        conv.state
            .macro_cache
            .insert(macro_def.name.clone(), output.clone());
    }
    output
}

/// Convert a LaTeX dimension to Typst
fn convert_dimension(dim: &str) -> String {
    let dim = dim.trim();

    // Handle LaTeX internal dimension macros
    // \p@ = 1pt, \z@ = 0pt
    // Strip trailing \par which can appear after \vskip
    let dim = dim
        .replace("\\p@", "pt")
        .replace("\\z@", "0pt")
        .replace("\\@plus", " ")
        .replace("\\@minus", " ")
        .replace("\\par", "");
    let dim = dim.trim();

    if let Some(rest) = dim.strip_prefix("\\stretch") {
        let rest = rest.trim();
        if let Some(arg) = rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            let value = arg.trim();
            if !value.is_empty() {
                return format!("{}fr", value);
            }
        }
        return "1fr".to_string();
    }

    if dim == "\\fill" || dim == "\\hfill" || dim == "\\vfill" {
        return "1fr".to_string();
    }

    // Handle \linewidth, \textwidth, etc.
    if dim.contains("\\linewidth") || dim.contains("\\textwidth") || dim.contains("\\columnwidth") {
        // Extract multiplier if present
        if let Some(mult) = dim.strip_suffix("\\linewidth") {
            let mult = mult.trim();
            if mult.is_empty() || mult == "1" {
                return "100%".to_string();
            }
            if let Ok(f) = mult.parse::<f32>() {
                return format!("{}%", (f * 100.0) as i32);
            }
        }
        if let Some(mult) = dim.strip_suffix("\\textwidth") {
            let mult = mult.trim();
            if mult.is_empty() || mult == "1" {
                return "100%".to_string();
            }
            if let Ok(f) = mult.parse::<f32>() {
                return format!("{}%", (f * 100.0) as i32);
            }
        }
        return "100%".to_string();
    }

    // Handle standard units
    let dim = dim
        .replace("\\fill", "1fr")
        .replace("\\hfill", "1fr");

    if dim.ends_with("pc") {
        let number = dim.trim_end_matches("pc").trim();
        if let Ok(value) = number.parse::<f32>() {
            let pts = value * 12.0;
            let mut out = format!("{:.4}", pts);
            while out.contains('.') && out.ends_with('0') {
                out.pop();
            }
            if out.ends_with('.') {
                out.pop();
            }
            return format!("{out}pt");
        }
    }

    // Already has a unit
    if dim.ends_with("pt")
        || dim.ends_with("em")
        || dim.ends_with("ex")
        || dim.ends_with("mm")
        || dim.ends_with("cm")
        || dim.ends_with("in")
        || dim.ends_with("pc")
        || dim.ends_with("bp")
        || dim.ends_with("%")
        || dim.ends_with("fr")
    {
        return dim;
    }

    // Just a number, assume pt
    if dim.parse::<f32>().is_ok() {
        return format!("{}pt", dim);
    }

    dim
}

/// Apply a text accent to a character
fn apply_text_accent(content: &str, accent: char) -> String {
    let c = content.chars().next().unwrap_or(' ');
    match accent {
        '`' => match c {
            'a' => "à".to_string(),
            'e' => "è".to_string(),
            'i' => "ì".to_string(),
            'o' => "ò".to_string(),
            'u' => "ù".to_string(),
            'A' => "À".to_string(),
            'E' => "È".to_string(),
            'I' => "Ì".to_string(),
            'O' => "Ò".to_string(),
            'U' => "Ù".to_string(),
            _ => content.to_string(),
        },
        '\'' => match c {
            'a' => "á".to_string(),
            'e' => "é".to_string(),
            'i' => "í".to_string(),
            'o' => "ó".to_string(),
            'u' => "ú".to_string(),
            'y' => "ý".to_string(),
            'A' => "Á".to_string(),
            'E' => "É".to_string(),
            'I' => "Í".to_string(),
            'O' => "Ó".to_string(),
            'U' => "Ú".to_string(),
            'Y' => "Ý".to_string(),
            _ => content.to_string(),
        },
        '^' => match c {
            'a' => "â".to_string(),
            'e' => "ê".to_string(),
            'i' => "î".to_string(),
            'o' => "ô".to_string(),
            'u' => "û".to_string(),
            'A' => "Â".to_string(),
            'E' => "Ê".to_string(),
            'I' => "Î".to_string(),
            'O' => "Ô".to_string(),
            'U' => "Û".to_string(),
            _ => content.to_string(),
        },
        '~' => match c {
            'a' => "ã".to_string(),
            'n' => "ñ".to_string(),
            'o' => "õ".to_string(),
            'A' => "Ã".to_string(),
            'N' => "Ñ".to_string(),
            'O' => "Õ".to_string(),
            _ => content.to_string(),
        },
        '"' => match c {
            'a' => "ä".to_string(),
            'e' => "ë".to_string(),
            'i' => "ï".to_string(),
            'o' => "ö".to_string(),
            'u' => "ü".to_string(),
            'y' => "ÿ".to_string(),
            'A' => "Ä".to_string(),
            'E' => "Ë".to_string(),
            'I' => "Ï".to_string(),
            'O' => "Ö".to_string(),
            'U' => "Ü".to_string(),
            _ => content.to_string(),
        },
        'u' => match c {
            'a' => "ă".to_string(),
            'e' => "ĕ".to_string(),
            'i' => "ĭ".to_string(),
            'o' => "ŏ".to_string(),
            'u' => "ŭ".to_string(),
            'A' => "Ă".to_string(),
            'E' => "Ĕ".to_string(),
            'I' => "Ĭ".to_string(),
            'O' => "Ŏ".to_string(),
            'U' => "Ŭ".to_string(),
            _ => content.to_string(),
        },
        'v' => match c {
            'c' => "č".to_string(),
            'C' => "Č".to_string(),
            's' => "š".to_string(),
            'S' => "Š".to_string(),
            'z' => "ž".to_string(),
            'Z' => "Ž".to_string(),
            'r' => "ř".to_string(),
            'R' => "Ř".to_string(),
            'e' => "ě".to_string(),
            'E' => "Ě".to_string(),
            'n' => "ň".to_string(),
            'N' => "Ň".to_string(),
            't' => "ť".to_string(),
            'T' => "Ť".to_string(),
            'd' => "ď".to_string(),
            'D' => "Ď".to_string(),
            _ => content.to_string(),
        },
        'k' => match c {
            'a' => "ą".to_string(),
            'A' => "Ą".to_string(),
            'e' => "ę".to_string(),
            'E' => "Ę".to_string(),
            _ => content.to_string(),
        },
        'H' => match c {
            'o' => "ő".to_string(),
            'O' => "Ő".to_string(),
            'u' => "ű".to_string(),
            'U' => "Ű".to_string(),
            _ => content.to_string(),
        },
        _ => content.to_string(),
    }
}

/// Apply cedilla
fn apply_cedilla(content: &str) -> String {
    let c = content.chars().next().unwrap_or(' ');
    match c {
        'c' => "ç".to_string(),
        'C' => "Ç".to_string(),
        _ => content.to_string(),
    }
}

/// Convert section heading with proper level
fn convert_section(conv: &mut LatexConverter, cmd: &CmdItem, level: u8, output: &mut String) {
    let title = conv
        .convert_required_arg(cmd, 0)
        .or_else(|| conv.get_required_arg(cmd, 0))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    if let Some(title) = title {
        // Normalize title: collapse internal newlines to single spaces
        // This ensures labels can attach to the heading line
        let normalized_title = title
            .lines()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        output.push('\n');
        for _ in 0..=level {
            output.push('=');
        }
        output.push(' ');
        output.push_str(&normalized_title);
        output.push('\n');
    } else {
        let raw = cmd.syntax().text().to_string();
        let (capture_mode, capture_depth) = if raw.ends_with('[') {
            (super::context::HeadingCaptureMode::Optional, 1)
        } else if raw.ends_with('{') {
            (super::context::HeadingCaptureMode::Required, 1)
        } else {
            (super::context::HeadingCaptureMode::None, 0)
        };
        let implicit_open = matches!(
            capture_mode,
            super::context::HeadingCaptureMode::Optional
                | super::context::HeadingCaptureMode::Required
        );
        conv.state.pending_heading = Some(PendingHeading {
            level,
            optional: None,
            required: None,
            capture_mode,
            capture_depth,
            capture_buffer: String::new(),
            implicit_open,
        });
    }
}

fn attach_label_to_heading(output: &mut String, label: &str) -> bool {
    let trimmed_len = output
        .trim_end_matches(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r')
        .len();
    if trimmed_len == 0 {
        return false;
    }
    let trimmed = &output[..trimmed_len];
    let line_start = trimmed.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let last_line = trimmed[line_start..].trim_start();
    if !last_line.starts_with('=') {
        return false;
    }

    output.truncate(trimmed_len);
    output.push(' ');
    output.push_str(&format!("<{}>", label));
    output.push('\n');
    true
}

fn parse_affiliation_keys(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter(|s| !s.eq_ignore_ascii_case("equal"))
        .filter(|s| *s != "*")
        .map(|s| s.to_string())
        .collect()
}

fn extract_first_braced_arg(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let mut depth = 0i32;
    for (idx, ch) in raw[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + idx;
                    return Some(raw[start + 1..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_first_bracket_arg(raw: &str) -> Option<String> {
    let start = raw.find('[')?;
    let mut depth = 0i32;
    for (idx, ch) in raw[start..].char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + idx;
                    return Some(raw[start + 1..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn package_list_contains(pkgs: &str, name: &str) -> bool {
    pkgs.split(',')
        .map(|s| s.trim())
        .any(|pkg| pkg == name)
}

fn apply_geometry_options(conv: &mut LatexConverter, options: &str) {
    for raw in options.split(',') {
        let opt = raw.trim();
        if opt.is_empty() {
            continue;
        }
        if let Some((key, value)) = opt.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "margin" => conv.state.page_margin.all = Some(value.to_string()),
                "left" => conv.state.page_margin.left = Some(value.to_string()),
                "right" => conv.state.page_margin.right = Some(value.to_string()),
                "top" => conv.state.page_margin.top = Some(value.to_string()),
                "bottom" => conv.state.page_margin.bottom = Some(value.to_string()),
                "hmargin" => {
                    conv.state.page_margin.left = Some(value.to_string());
                    conv.state.page_margin.right = Some(value.to_string());
                }
                "vmargin" => {
                    conv.state.page_margin.top = Some(value.to_string());
                    conv.state.page_margin.bottom = Some(value.to_string());
                }
                "paper" => {
                    conv.state.page_paper = Some(value.to_string());
                }
                _ => {}
            }
            continue;
        }
        if opt.ends_with("paper") && opt.len() > "paper".len() {
            let paper = opt.trim_end_matches("paper");
            if !paper.is_empty() {
                conv.state.page_paper = Some(paper.to_string());
            }
        }
    }
}

fn apply_length_setting(conv: &mut LatexConverter, target: &str, value: &str) {
    let mut name = target.trim().trim_start_matches('\\').to_string();
    name.retain(|c| c.is_ascii_alphabetic());
    let val = value.trim().trim_matches(|c| c == '{' || c == '}');
    let converted = convert_dimension(val);
    if name.contains("parskip") {
        conv.state.par_skip = Some(converted);
    } else if name.contains("parindent") {
        conv.state.par_indent = Some(converted);
    }
}

fn apply_line_spread(conv: &mut LatexConverter, value: &str) {
    let val = value.trim();
    let Ok(scale) = val.parse::<f32>() else {
        return;
    };
    if scale <= 1.0 {
        conv.state.line_spacing = None;
        return;
    }
    let leading = scale - 1.0;
    conv.state.line_spacing = Some(format!("{:.3}em", leading));
}

fn apply_fancy_head(conv: &mut LatexConverter, cmd: &CmdItem) {
    let opt = conv
        .get_optional_arg(cmd, 0)
        .or_else(|| extract_first_bracket_arg(&cmd.syntax().text().to_string()))
        .unwrap_or_default();
    let content = conv
        .get_required_arg_with_braces(cmd, 0)
        .or_else(|| extract_first_braced_arg(&cmd.syntax().text().to_string()))
        .unwrap_or_default();
    let text = convert_caption_text(&content);
    if opt.trim().is_empty() {
        return;
    }
    let key = opt.trim().to_uppercase();
    if key.contains('L') {
        conv.state.header.left = Some(text.trim().to_string());
    }
    if key.contains('C') {
        conv.state.header.center = Some(text.trim().to_string());
    }
    if key.contains('R') {
        conv.state.header.right = Some(text.trim().to_string());
    }
}

fn apply_titleformat(conv: &mut LatexConverter, cmd: &CmdItem) {
    let target = conv.get_required_arg(cmd, 0).unwrap_or_default();
    let format = conv.get_required_arg(cmd, 1).unwrap_or_default();
    let level = match target.trim().trim_start_matches('\\') {
        "section" => Some(1),
        "subsection" => Some(2),
        "subsubsection" => Some(3),
        "paragraph" => Some(4),
        "subparagraph" => Some(5),
        _ => None,
    };
    let Some(level) = level else {
        return;
    };
    let style = parse_heading_style(&format);
    conv.state.heading_styles.insert(level, style);
}

fn parse_heading_style(format: &str) -> super::context::HeadingStyleDef {
    let mut style = super::context::HeadingStyleDef::default();
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
