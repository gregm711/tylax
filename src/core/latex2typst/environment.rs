//! Environment handling for LaTeX to Typst conversion
//!
//! This module handles LaTeX environments like figure, table, itemize, equation, etc.

use mitex_parser::syntax::{CmdItem, EnvItem, SyntaxElement, SyntaxKind, SyntaxNode};
use rowan::ast::AstNode;
use std::fmt::Write;
use std::time::Instant;

use super::context::{ConversionMode, EnvironmentContext, LatexConverter, TemplateKind};
use super::table::{parse_with_grid_parser, CellAlign};
use super::utils::{convert_caption_text, escape_typst_string, sanitize_label, strip_label_from_text};
use crate::data::constants::{CodeBlockOptions, TheoremStyle, LANGUAGE_MAP, THEOREM_TYPES};
use crate::utils::loss::{LossKind, LOSS_MARKER_PREFIX};

/// Convert a LaTeX environment
pub fn convert_environment(conv: &mut LatexConverter, elem: SyntaxElement, output: &mut String) {
    let node = match &elem {
        SyntaxElement::Node(n) => n.clone(),
        _ => return,
    };

    let env = match EnvItem::cast(node.clone()) {
        Some(e) => e,
        None => return,
    };

    let env_name = env.name_tok().map(|t| t.text().to_string());
    let env_str = env_name.as_deref().unwrap_or("");
    let env_trim = env_str.trim().trim_end_matches('*');
    if conv.state.profile_enabled {
        conv.state.profile_last_env = Some(env_trim.to_string());
    }
    let profile_enabled = conv.state.profile_enabled;
    let env_start = if profile_enabled { Some(Instant::now()) } else { None };

    match env_trim {
        // Document environment - marks end of preamble
        "document" => {
            conv.state.in_preamble = false;
            conv.visit_env_content(&node, output);
        }

        // Figure-like environments
        "figure" | "listing" | "sidewaysfigure" | "wrapfigure" | "marginfigure" => {
            convert_figure(conv, &node, output);
        }

        // Table environment
        "table" | "wraptable" => {
            convert_table(conv, &node, output);
        }

        // Tabular environment
        "tabular" | "tabularx" | "longtable" | "longtab" | "longtabu" | "array" => {
            convert_tabular(conv, &node, output);
        }

        // List environments
        "itemize" | "compactitem" => {
            conv.state.push_env(EnvironmentContext::Itemize);
            output.push('\n');
            conv.visit_env_content(&node, output);
            conv.state.pop_env();
            output.push('\n');
        }
        "enumerate" | "compactenum" => {
            conv.state.push_env(EnvironmentContext::Enumerate);
            output.push('\n');
            conv.visit_env_content(&node, output);
            conv.state.pop_env();
            output.push('\n');
        }
        "description" => {
            conv.state.push_env(EnvironmentContext::Description);
            output.push('\n');
            conv.visit_env_content(&node, output);
            conv.state.pop_env();
            output.push('\n');
        }
        "acronym" => {
            conv.state.push_env(EnvironmentContext::Description);
            output.push('\n');
            conv.visit_env_content(&node, output);
            conv.state.pop_env();
            output.push('\n');
        }
        "symbollist" => {
            conv.state.push_env(EnvironmentContext::Description);
            output.push('\n');
            conv.visit_env_content(&node, output);
            conv.state.pop_env();
            output.push('\n');
        }
        "anexosenv" | "apendicesenv" => {
            // Appendix wrapper environments: emit content.
            conv.visit_env_content(&node, output);
        }
        "list" => {
            // Generic list environment; treat as itemize
            conv.state.push_env(EnvironmentContext::Itemize);
            output.push('\n');
            conv.visit_env_content(&node, output);
            conv.state.pop_env();
            output.push('\n');
        }
        "multicols" => {
            // Ignore column layout; emit content
            conv.visit_env_content(&node, output);
        }
        "spacing" | "onehalfspace" | "singlespace" | "doublespace" | "justifying" | "justify"
        | "LARGE" => {
            // setspace environment: ignore spacing, emit content.
            conv.visit_env_content(&node, output);
        }
        "vplace" => {
            // Memoir vertical placement: approximate with flexible vertical space.
            output.push_str("\n#v(1fr)\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n#v(1fr)\n");
        }
        // Thesis/front-matter wrappers: emit content or light headings.
        "address" | "mainf" | "bibliof" | "publishedcontent" | "bibunit"
        | "romanpages" | "mclistof" | "mccorrection" => {
            conv.visit_env_content(&node, output);
        }
        "abstractseparate" => {
            output.push_str("\n= Abstract\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "abstractpage" | "thesisabstract" => {
            output.push_str("\n= Abstract\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "Abstract" => {
            output.push_str("\n= Abstract\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "Resumo" => {
            output.push_str("\n= Abstract\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "refsection" => {
            // Bibliography subsection wrappers (biblatex): emit content.
            conv.visit_env_content(&node, output);
        }
        "textblock" | "thesis" | "body" => {
            // textpos/thesis wrappers: ignore layout, emit content.
            conv.visit_env_content(&node, output);
        }
        "adjustwidth" => {
            // changepage: ignore margins, emit content.
            conv.visit_env_content(&node, output);
        }
        "thesisacknowledgments" | "thesisacknowledgements" | "thankpage" => {
            output.push_str("\n= Acknowledgments\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "AgradecimentosAutorI" | "AgradecimentosAutorII" => {
            output.push_str("\n= Acknowledgments\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "thesisdeclaration" => {
            output.push_str("\n= Declaration\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "synopsis" => {
            output.push_str("\n= Synopsis\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "researchpage" => {
            output.push_str("\n= Research\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "zusammenfassung" => {
            output.push_str("\n= Zusammenfassung\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "ozet" => {
            output.push_str("\n= Ozet\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "otherlanguage" => {
            conv.visit_env_content(&node, output);
        }
        "abbrv" => {
            output.push_str("\n= Abbreviations\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "vita" => {
            output.push_str("\n= Vita\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "filecontents" => {
            // LaTeX writes these to files; ignore output.
            let mut _scratch = String::new();
            conv.visit_env_content(&node, &mut _scratch);
        }

        // ICML author list (metadata only)
        "icmlauthorlist" => {
            let mut scratch = String::new();
            conv.visit_env_content(&node, &mut scratch);
        }

        // Math environments
        "equation" => {
            convert_equation(conv, &node, env_str, output);
        }
        "linenomath" => {
            conv.visit_env_content(&node, output);
        }
        "align" | "aligned" | "alignat" | "flalign" | "eqnarray" => {
            convert_align(conv, &node, env_str, output);
        }
        "gather" => {
            convert_gather(conv, &node, env_str, output);
        }
        "gathered" => {
            convert_gathered(conv, &node, output);
        }
        "multline" => {
            convert_multline(conv, &node, env_str, output);
        }
        "split" => {
            // split is usually inside equation, just process content
            conv.state.push_env(EnvironmentContext::Align);
            let mut content = String::new();
            conv.visit_env_content(&node, &mut content);
            conv.state.pop_env();
            output.push_str(&content);
        }

        // Matrix environments
        "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix" | "smallmatrix" => {
            convert_matrix(conv, &node, env_str, output);
        }

        // Cases
        "cases" | "dcases" | "rcases" => {
            convert_cases(conv, &node, output);
        }

        // Code/verbatim environments
        "verbatim" | "verbatim*" | "Verbatim" | "alltt" => {
            convert_verbatim(conv, &node, output);
        }
        "lstlisting" => {
            convert_lstlisting(conv, &node, output);
        }
        // Knitr / shaded output blocks: keep raw to avoid breaking code-like content.
        "knitrout"
        | "kframe"
        | "Shaded"
        | "shaded"
        | "Highlighting"
        | "Sinput"
        | "Soutput"
        | "Schunk" => {
            convert_verbatim(conv, &node, output);
        }
        "Large" | "large" | "scriptsize" | "small"
        | "singlespacing" | "onehalfspacing" | "doublespacing" => {
            conv.visit_env_content(&node, output);
        }

        // Custom boxes and pages
        "titlepage" => {
            convert_titlepage(conv, &node, output);
        }
        "tcolorbox" | "tcolorbox*" | "OxWarningBox" | "OxInfoBox" => {
            convert_boxed_env(conv, &node, output);
        }
        "program" => {
            convert_program(conv, &node, output);
        }
        "minted" => {
            convert_minted(conv, &node, output);
        }
        "savequote" => {
            convert_savequote(conv, &node, output);
        }
        "frontmatter" | "sloppypar" | "fullwidth" | "landscape" | "onecolumn" => {
            conv.visit_env_content(&node, output);
        }
        "docspec" | "docspecdef" => {
            output.push_str("#block[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }
        "nomenclature" | "nomenclature*" => {
            output.push_str("\n= Nomenclature\n\n");
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "dedication" | "acknowledgements" | "acknowledgments" | "acknowledgement"
        | "ack" | "acks" | "DedicatoriaAutorI" | "DedicatoriaAutorII" => {
            let title = match env_str {
                "dedication" => "Dedication",
                "acknowledgements" => "Acknowledgements",
                _ => "Acknowledgments",
            };
            let _ = writeln!(output, "\n= {}\n", title);
            conv.visit_env_content(&node, output);
            output.push('\n');
        }
        "Epigrafe" => {
            let mut content = String::new();
            conv.visit_env_content(&node, &mut content);
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let _ = write!(output, "\n#quote[{}]\n", trimmed);
            }
        }
        "sidewaystable" => {
            convert_table(conv, &node, output);
        }

        // ACM metadata XML block (ignore content)
        "CCSXML" => {
            let mut _scratch = String::new();
            conv.visit_env_content(&node, &mut _scratch);
        }

        // Prompt blocks used in some datasets
        "prompt" => {
            let mut content = String::new();
            conv.visit_env_content(&node, &mut content);
            if !content.trim().is_empty() {
                output.push_str("\n#block[\n");
                output.push_str(content.trim());
                output.push_str("\n]\n");
            }
        }

        // TikZ
        "tikzpicture" => {
            convert_tikz(conv, &node, output);
        }

        // Cryptocode-style gameproof blocks: keep as raw to avoid invalid math/text mixes
        "gameproof" => {
            convert_gameproof(conv, &node, output);
        }

        // Thmtools restatable environment: treat as theorem-like.
        "restatable" => {
            let kind_raw = conv
                .get_env_required_arg(&node, 0)
                .unwrap_or_else(|| "theorem".to_string());
            let kind_trim = kind_raw.trim();
            let kind_lower = kind_trim.to_lowercase();
            let env_name = if THEOREM_TYPES.contains_key(kind_lower.as_str()) {
                kind_lower.as_str()
            } else {
                kind_trim
            };
            convert_theorem(conv, &node, env_name, output);
        }

        // Theorem-like environments
        "theorem" | "lemma" | "proposition" | "corollary" | "definition" | "example" | "remark"
        | "proof" | "conjecture" | "claim" | "fact" | "observation" | "property" | "question"
        | "problem" | "solution" | "answer" | "exercise" | "assumption" | "hypothesis"
        | "notation" | "conclusion"
        // Common short forms
        | "thm" | "lem" | "prop" | "rem" | "cor" | "defn"
        // Custom theorem-like environments from packages
        | "enumthm" => {
            convert_theorem(conv, &node, env_trim, output);
        }

        // IEEE keywords environment
        "IEEEkeywords" | "keywords" | "keyword" => {
            let mut buffer = String::new();
            conv.visit_env_content(&node, &mut buffer);
            let text = buffer.trim();
            if !text.is_empty() {
                let cleaned = convert_caption_text(text);
                conv.state.keywords = cleaned
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        // IEEE biography blocks
        "IEEEbiography" | "IEEEbiographynophoto" => {
            let name = conv
                .get_env_required_arg(&node, 0)
                .map(|raw| convert_caption_text(&raw))
                .unwrap_or_default();
            output.push_str("\n#block[\n");
            if !name.trim().is_empty() {
                let _ = writeln!(output, "#text(weight: \"bold\")[{}]", name.trim());
                output.push_str("#v(0.25em)\n");
            }
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }

        // Wrapper environments that should keep inner content
        "subequations" | "savenotes" => {
            output.push('\n');
            conv.visit_env_content(&node, output);
            output.push('\n');
        }

        // Custom shaded blocks
        "greycustomblock" => {
            output.push_str("\n#block(fill: luma(245), stroke: luma(200), inset: 0.6em, radius: 4pt)[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }

        // Comment environment (ignore content)
        "comment" => {
            // Ignore everything inside.
        }

        // Quote environments
        "quote" | "quotation" | "customblockquote" => {
            output.push_str("\n#quote(block: true)[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }
        "verse" => {
            output.push_str("#block(inset: (left: 2em))[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }

        // Abstract
        "abstract" => {
            if conv.state.template_kind == Some(TemplateKind::Ieee) {
                let mut buffer = String::new();
                conv.visit_env_content(&node, &mut buffer);
                conv.state.abstract_text = Some(buffer.trim().to_string());
            } else {
                output.push_str("\n#block(width: 100%, inset: 1em)[\n");
                output.push_str("  #align(center)[#text(weight: \"bold\")[Abstract]]\n  ");
                conv.visit_env_content(&node, output);
                output.push_str("\n]\n");
            }
        }

        // Center, flushleft, flushright
        "center" => {
            output.push_str("#align(center)[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }
        "flushleft" | "raggedright" => {
            output.push_str("#align(left)[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }
        "flushright" | "raggedleft" => {
            output.push_str("#align(right)[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }

        // Minipage
        "minipage" => {
            let width = conv
                .get_env_required_arg(&node, 0)
                .unwrap_or("100%".to_string());
            let _ = writeln!(output, "#block(width: {})[", convert_dimension(&width));
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }

        // Bibliography
        "thebibliography" | "mcitethebibliography" => {
            convert_bibliography(conv, &node, output);
        }

        // Appendix
        "appendix" | "appendices" => {
            output.push_str("\n// Appendix\n");
            conv.visit_env_content(&node, output);
        }

        // Frame (beamer)
        "frame" => {
            convert_frame(conv, &node, output);
        }

        // Columns (beamer)
        "columns" => {
            output.push_str("#grid(columns: 2)[\n");
            conv.visit_env_content(&node, output);
            output.push_str("\n]\n");
        }
        "column" => {
            // Individual column in columns environment
            conv.visit_env_content(&node, output);
        }

        // Subfigure
        "subfigure" => {
            convert_subfigure(conv, &node, output);
        }

        // Algorithm
        "algorithm" | "algorithmic" | "algorithm2e" => {
            convert_algorithm(conv, &node, output);
        }

        // Unknown environments - pass through content
        _ => {
            let env_lower = env_trim.to_lowercase();
            // Check if it's a theorem-like environment defined by user or known
            if conv.state.custom_theorems.contains_key(env_trim)
                || THEOREM_TYPES.contains_key(env_trim)
            {
                convert_theorem(conv, &node, env_trim, output);
            } else if THEOREM_TYPES.contains_key(env_lower.as_str()) {
                convert_theorem(conv, &node, &env_lower, output);
            } else {
                let loss_id = conv.record_loss(
                    LossKind::UnknownEnvironment,
                    Some(env_trim.to_string()),
                    format!("Unknown environment {}", env_trim),
                    Some(node.text().to_string()),
                    Some("text".to_string()),
                );
                let loss_marker = format!(" /* {}{} */ ", LOSS_MARKER_PREFIX, loss_id);
                // Just process content
                let _ = writeln!(output, "{} /* Begin {} */", loss_marker, env_trim);
                conv.visit_env_content(&node, output);
                let _ = write!(output, "\n/* End {} */\n", env_trim);
            }
        }
    }

    if let Some(start) = env_start {
        let elapsed = start.elapsed().as_secs_f64();
        if elapsed >= 0.05 {
            eprintln!("[tylax] env {} total {:.3}s", env_trim, elapsed);
        }
    }
}

fn convert_savequote(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Savequote);
    let mut content = String::new();
    conv.visit_env_content(node, &mut content);
    conv.state.pop_env();
    output.push_str("\n#quote[\n");
    output.push_str(content.trim());
    output.push_str("\n]\n");
}

fn convert_titlepage(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    output.push_str("\n#pagebreak()\n");
    let mut content = String::new();
    conv.visit_env_content(node, &mut content);
    output.push_str(content.trim());
    output.push_str("\n#pagebreak()\n");
}

fn convert_boxed_env(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    let mut content = String::new();
    conv.visit_env_content(node, &mut content);
    output.push_str("\n#block(stroke: 0.6pt, inset: 6pt)[\n");
    output.push_str(content.trim());
    output.push_str("\n]\n");
}

fn convert_program(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    let mut caption_cmd: Option<CmdItem> = None;
    let mut label_text = String::new();
    let mut content = String::new();

    for child in node.children_with_tokens() {
        if let SyntaxElement::Node(n) = &child {
            if let Some(cmd) = CmdItem::cast(n.clone()) {
                if let Some(name_tok) = cmd.name_tok() {
                    let name = name_tok.text();
                    if name == "\\caption" {
                        caption_cmd = Some(cmd.clone());
                    } else if name == "\\label" {
                        if let Some(lbl) = conv.get_required_arg(&cmd, 0) {
                            label_text = lbl;
                        }
                    }
                }
            }
        }
    }

    conv.visit_env_content(node, &mut content);

    output.push_str("\n#figure(");
    if let Some(ref cmd) = caption_cmd {
        if let Some(cap) = conv.get_converted_required_arg(cmd, 0) {
            let _ = writeln!(output, "\n  caption: [{}],", cap);
        }
    }
    output.push_str(")[\n");
    output.push_str(content.trim());
    output.push_str("\n] ");
    if !label_text.is_empty() {
        let _ = write!(output, "<{}>", sanitize_label(&label_text));
    }
    output.push('\n');
}

// =============================================================================
// Environment conversion functions
// =============================================================================

/// Convert a figure environment
fn convert_figure(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Figure);

    // Find image and caption using AST
    let mut image_expr: Option<String> = None;
    let mut caption_cmd: Option<CmdItem> = None;
    let mut label_text = String::new();
    let mut subfigs: Vec<String> = Vec::new();

    for child in node.children_with_tokens() {
        if let SyntaxElement::Node(n) = &child {
            if let Some(cmd) = CmdItem::cast(n.clone()) {
                if let Some(name_tok) = cmd.name_tok() {
                    let name = name_tok.text();
                    if name == "\\subcaptionbox" {
                        let sub = render_subcaptionbox(conv, &cmd);
                        if !sub.trim().is_empty() {
                            subfigs.push(sub);
                        }
                        continue;
                    }
                    if name == "\\includegraphics" {
                        if let Some(path) = conv.get_required_arg(&cmd, 0) {
                            let trimmed = path.trim();
                            if !trimmed.is_empty()
                                && !trimmed.contains('\\')
                                && !trimmed.contains('{')
                                && !trimmed.contains('}')
                            {
                                image_expr = Some(format!("  image(\"{}\")", trimmed));
                            }
                        }
                    } else if name == "\\caption" {
                        // Store the command for later conversion
                        caption_cmd = Some(cmd.clone());
                    } else if name == "\\label" {
                        if let Some(lbl) = conv.get_required_arg(&cmd, 0) {
                            label_text = lbl;
                        }
                    }
                }
            }
        }
    }

    output.push_str("\n#figure(\n");
    let has_image = image_expr.is_some();
    let mut wrote_arg = has_image;
    let has_subfigs = !subfigs.is_empty();
    if !has_image && !has_subfigs {
        output.push_str("  []");
        wrote_arg = true;
    }
    if has_image && !has_subfigs {
        if let Some(expr) = image_expr.take() {
            output.push_str(&expr);
        }
    }

    // Convert caption content (may contain math like $\downarrow$)
    if let Some(ref cmd) = caption_cmd {
        if let Some(cap) = conv.get_converted_required_arg(cmd, 0) {
            if wrote_arg {
                output.push_str(",\n");
            } else {
                output.push('\n');
            }
            let _ = write!(output, "  caption: [{}]", cap);
            wrote_arg = true;
        }
    }

    if wrote_arg {
        output.push('\n');
    }

    output.push(')');

    if has_subfigs {
        output.push_str("[\n");
        if subfigs.len() > 1 {
            let columns = if subfigs.len() >= 3 { 3 } else { 2 };
            let _ = writeln!(output, "#grid(columns: {}, gutter: 1em)[", columns);
            for sub in &subfigs {
                let _ = writeln!(output, "  {},", sub.trim());
            }
            output.push_str("]\n");
        } else if let Some(first) = subfigs.first() {
            output.push_str(first.trim());
            output.push('\n');
        }
        output.push_str("]\n");
    }

    if !label_text.is_empty() {
        let _ = write!(output, " <{}>", sanitize_label(&label_text));
    }

    output.push('\n');

    conv.state.pop_env();
}

fn render_subcaptionbox(conv: &mut LatexConverter, cmd: &CmdItem) -> String {
    let raw_caption = conv.get_required_arg_with_braces(cmd, 0).unwrap_or_default();
    let (caption, label) = strip_label_from_text(&raw_caption);
    let caption_text = convert_caption_text(&caption);
    let content = conv.convert_required_arg(cmd, 1).unwrap_or_default();

    let mut out = String::new();
    out.push_str("#figure(kind: \"subfigure\", supplement: none");
    if !caption_text.trim().is_empty() {
        let _ = write!(out, ", caption: [{}]", caption_text.trim());
    }
    out.push_str(")[\n");
    if !content.trim().is_empty() {
        out.push_str(content.trim());
        out.push('\n');
    }
    out.push_str("]");
    if let Some(lbl) = label {
        let clean = sanitize_label(&lbl);
        if !clean.is_empty() {
            let _ = write!(out, " <{}>", clean);
        }
    }
    out
}

/// Convert a table environment
fn convert_table(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Table);

    let mut caption_cmd: Option<CmdItem> = None;
    let mut label_text = String::new();
    let mut table_content = String::new();
    let mut notes_content = String::new();
    // First pass: extract caption, label, and tabular content using AST nodes only.
    // Some tables wrap tabular/caption inside helper envs (e.g., threeparttable),
    // so scan descendants instead of only direct children.
    for descendant in node.descendants() {
        if let Some(cmd) = CmdItem::cast(descendant.clone()) {
            if let Some(name_tok) = cmd.name_tok() {
                let name = name_tok.text();
                if caption_cmd.is_none() && name == "\\caption" {
                    caption_cmd = Some(cmd.clone());
                } else if label_text.is_empty() && name == "\\label" {
                    if let Some(lbl) = conv.get_required_arg(&cmd, 0) {
                        label_text = lbl;
                    }
                }
            }
        }
        if table_content.is_empty() {
            if let Some(env) = EnvItem::cast(descendant.clone()) {
                if env
                    .name_tok()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default()
                    .starts_with("tabular")
                {
                    // convert_tabular handles its own push/pop of Tabular context
                    convert_tabular(conv, &descendant, &mut table_content);
                }
            }
        }
        if notes_content.is_empty() {
            if let Some(env) = EnvItem::cast(descendant.clone()) {
                let env_name = env
                    .name_tok()
                    .map(|t| t.text().to_string())
                    .unwrap_or_default();
                if env_name == "tablenotes" || env_name == "tablenotes*" {
                    conv.state.push_env(EnvironmentContext::Description);
                    conv.visit_env_content(&descendant, &mut notes_content);
                    conv.state.pop_env();
                }
            }
        }
    }

    // Build properly formatted figure
    output.push_str("\n#figure(");

    // Convert caption content (may contain math)
    if let Some(ref cmd) = caption_cmd {
        if let Some(cap) = conv.get_converted_required_arg(cmd, 0) {
            let _ = writeln!(output, "\n  caption: [{}],", cap);
        }
    }

    output.push_str(")[\n");
    output.push_str(&table_content);
    if !notes_content.trim().is_empty() {
        output.push('\n');
        output.push_str(notes_content.trim());
        output.push('\n');
    }
    output.push_str("\n] ");

    if !label_text.is_empty() {
        let _ = write!(output, "<{}>", sanitize_label(&label_text));
    }

    output.push('\n');

    conv.state.pop_env();
}

/// Convert a tabular environment using the state-aware grid parser
fn convert_tabular(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Tabular);

    // Save current mode and force Text mode for tabular content
    let prev_mode = conv.state.mode;
    let in_math = matches!(prev_mode, ConversionMode::Math);
    if !in_math {
        conv.state.mode = ConversionMode::Text;
    }

    // Get column specification from the environment's first required argument
    let col_spec = get_tabular_col_spec(node).unwrap_or_default();
    let columns = parse_column_spec(&col_spec);

    // Convert column specs to CellAlign
    let alignments: Vec<CellAlign> = columns
        .iter()
        .map(|c| match c.as_str() {
            "l" => CellAlign::Left,
            "r" => CellAlign::Right,
            "c" => CellAlign::Center,
            _ => CellAlign::Auto,
        })
        .collect();

    // Collect table content
    let mut content = String::new();
    conv.visit_env_content(node, &mut content);

    // Restore previous mode
    conv.state.mode = prev_mode;

    if in_math {
        let math_output = render_math_matrix(&content);
        output.push_str(&math_output);
    } else {
        // Use the new grid parser
        let parse_start = if conv.state.profile_enabled {
            Some(Instant::now())
        } else {
            None
        };
        let typst_output = parse_with_grid_parser(&content, alignments);
        if let Some(start) = parse_start {
            let elapsed = start.elapsed().as_secs_f64();
            if elapsed >= 0.05 {
                eprintln!(
                    "[tylax] tabular parse {:.3}s (len={})",
                    elapsed,
                    content.len()
                );
            }
        }
        output.push_str(&typst_output);
    }

    conv.state.pop_env();
}

/// Render a tabular/array-like environment into a Typst math matrix.
fn render_math_matrix(raw: &str) -> String {
    let mut rows_out: Vec<String> = Vec::new();
    for row in raw.split("|||ROW|||") {
        let mut row_str = row.trim().trim_matches(',').to_string();
        if row_str.contains("|||HLINE|||") {
            row_str = row_str.replace("|||HLINE|||", "");
        }
        if row_str.trim().is_empty() {
            continue;
        }
        if row_str.contains("table.hline") || row_str.contains("table.cline") {
            continue;
        }
        let mut cells_out: Vec<String> = Vec::new();
        for cell in row_str.split("|||CELL|||") {
            let mut cell_str = cell.trim().trim_matches(',').to_string();
            if cell_str.contains("|||HLINE|||") {
                cell_str = cell_str.replace("|||HLINE|||", "");
            }
            let cell_str = cell_str.trim();
            if cell_str.contains("table.hline") || cell_str.contains("table.cline") {
                continue;
            }
            if cell_str.is_empty() {
                cells_out.push("zws".to_string());
            } else {
                cells_out.push(cell_str.to_string());
            }
        }
        if !cells_out.is_empty() {
            rows_out.push(cells_out.join(", "));
        }
    }

    if rows_out.is_empty() {
        "mat()".to_string()
    } else {
        format!("mat({})", rows_out.join("; "))
    }
}

/// Convert an equation environment
fn convert_equation(
    conv: &mut LatexConverter,
    node: &SyntaxNode,
    env_name: &str,
    output: &mut String,
) {
    conv.state.push_env(EnvironmentContext::Equation);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;

    // Check if this is a starred (unnumbered) equation
    let is_starred = env_name.ends_with('*');

    // Extract label first using AST
    let mut label = String::new();
    for child in node.children_with_tokens() {
        if let SyntaxElement::Node(n) = &child {
            if let Some(cmd) = CmdItem::cast(n.clone()) {
                if let Some(name_tok) = cmd.name_tok() {
                    if name_tok.text() == "\\label" {
                        if let Some(lbl) = conv.get_required_arg(&cmd, 0) {
                            label = lbl;
                        }
                    }
                }
            }
        }
    }

    // Collect math content into a buffer for post-processing
    let mut math_content = String::new();
    conv.visit_env_content(node, &mut math_content);

    // Apply math cleanup
    let cleaned = conv.cleanup_math_spacing(&math_content);

    output.push_str("#math.equation(block: true");
    if is_starred {
        output.push_str(", numbering: none");
    }
    output.push_str(")[\n$ ");
    output.push_str(cleaned.trim());
    output.push_str(" $\n]");
    if !label.is_empty() {
        let _ = write!(output, " <{}>", sanitize_label(&label));
    }
    output.push('\n');

    conv.state.mode = prev_mode;
    conv.state.pop_env();
}

/// Convert an align environment
fn convert_align(
    conv: &mut LatexConverter,
    node: &SyntaxNode,
    env_name: &str,
    output: &mut String,
) {
    conv.state.push_env(EnvironmentContext::Align);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;
    if conv.state.profile_enabled {
        eprintln!("[tylax] align enter");
    }

    // Only add $ for non-aligned (aligned is usually inside math mode already)
    let is_inner = env_name == "aligned";

    // Check if this is a starred (unnumbered) environment
    let is_starred = env_name.ends_with('*');

    // Extract label first using AST (for numbered align environments)
    let mut label = String::new();
    for child in node.children_with_tokens() {
        if let SyntaxElement::Node(n) = &child {
            if let Some(cmd) = CmdItem::cast(n.clone()) {
                if let Some(name_tok) = cmd.name_tok() {
                    if name_tok.text() == "\\label" {
                        if let Some(lbl) = conv.get_required_arg(&cmd, 0) {
                            label = lbl;
                        }
                    }
                }
            }
        }
    }

    // Collect math content into a buffer for post-processing
    let mut math_content = String::new();
    let visit_start = if conv.state.profile_enabled {
        Some(Instant::now())
    } else {
        None
    };
    conv.visit_env_content(node, &mut math_content);
    let visit_secs = visit_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);

    // Apply math cleanup
    let cleanup_start = if conv.state.profile_enabled {
        Some(Instant::now())
    } else {
        None
    };
    let cleaned = conv.cleanup_math_spacing(&math_content);
    let cleanup_secs = cleanup_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
    if conv.state.profile_enabled {
        eprintln!(
            "[tylax] align len={} visit={:.3}s cleanup={:.3}s",
            math_content.len(),
            visit_secs,
            cleanup_secs
        );
    }

    if !is_inner {
        output.push_str("#math.equation(block: true");
        if is_starred {
            output.push_str(", numbering: none");
        }
        output.push_str(")[\n$ ");
        output.push_str(cleaned.trim());
        output.push_str(" $\n]");
        if !label.is_empty() {
            let _ = write!(output, " <{}>", sanitize_label(&label));
        }
        output.push('\n');
    } else {
        output.push_str(&cleaned);
    }

    conv.state.mode = prev_mode;
    conv.state.pop_env();
}

/// Convert a gather environment
fn convert_gather(
    conv: &mut LatexConverter,
    node: &SyntaxNode,
    env_name: &str,
    output: &mut String,
) {
    conv.state.push_env(EnvironmentContext::Equation);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;

    let is_starred = env_name.ends_with('*');

    let mut content = String::new();
    conv.visit_env_content(node, &mut content);

    conv.state.mode = prev_mode;
    conv.state.pop_env();

    let processed = if content.len() > 10_000 {
        conv.cleanup_math_spacing(&content)
    } else {
        conv.postprocess_math(content)
    };

    let mut heading = String::new();
    heading.push_str("#math.equation(block: true");
    if is_starred {
        heading.push_str(", numbering: none");
    }
    heading.push_str(")[\n$ ");
    heading.push_str(processed.trim());
    heading.push_str(" $\n]\n");
    let _ = write!(output, "{}", heading);
}

/// Convert a multline environment
fn convert_multline(
    conv: &mut LatexConverter,
    node: &SyntaxNode,
    env_name: &str,
    output: &mut String,
) {
    conv.state.push_env(EnvironmentContext::Equation);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;

    let is_starred = env_name.ends_with('*');

    let mut content = String::new();
    conv.visit_env_content(node, &mut content);

    conv.state.mode = prev_mode;
    conv.state.pop_env();

    let processed = if content.len() > 10_000 {
        conv.cleanup_math_spacing(&content)
    } else {
        conv.postprocess_math(content)
    };

    let mut heading = String::new();
    heading.push_str("#math.equation(block: true");
    if is_starred {
        heading.push_str(", numbering: none");
    }
    heading.push_str(")[\n$ ");
    heading.push_str(processed.trim());
    heading.push_str(" $\n]\n");
    let _ = write!(output, "{}", heading);
}

/// Convert a gathered environment (inner math)
fn convert_gathered(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Align);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;

    let mut content = String::new();
    conv.visit_env_content(node, &mut content);

    conv.state.mode = prev_mode;
    conv.state.pop_env();

    let cleaned = conv.cleanup_math_spacing(&content);
    output.push_str(cleaned.trim());
}

/// Convert a matrix environment
fn convert_matrix(
    conv: &mut LatexConverter,
    node: &SyntaxNode,
    env_name: &str,
    output: &mut String,
) {
    conv.state.push_env(EnvironmentContext::Matrix);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;

    let mut content = String::new();
    conv.visit_env_content(node, &mut content);

    conv.state.mode = prev_mode;
    conv.state.pop_env();

    // Determine delimiter type
    // For plain "matrix" environment, use delim: #none
    // For others, use the appropriate delimiter string
    let delim = match env_name {
        "pmatrix" => Some("("),
        "bmatrix" => Some("["),
        "Bmatrix" => Some("{"),
        "vmatrix" => Some("|"),
        "Vmatrix" => Some("‖"), // Use double bar Unicode character for Typst
        "smallmatrix" | "matrix" => None,
        _ => None,
    };

    // Clean up content - remove zws markers and format
    let content = content
        .replace("zws ;", ";")
        .replace("zws, ", ", ")
        .trim()
        .to_string();

    match delim {
        Some(d) => {
            let _ = write!(output, "mat(delim: \"{}\", {}) ", d, content);
        }
        None => {
            let _ = write!(output, "mat(delim: #none, {}) ", content);
        }
    }
}

/// Convert a cases environment
fn convert_cases(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Cases);
    let prev_mode = conv.state.mode;
    conv.state.mode = ConversionMode::Math;

    let mut content = String::new();
    conv.visit_env_content(node, &mut content);

    conv.state.mode = prev_mode;
    conv.state.pop_env();

    // Format as cases
    let content = content.trim();
    let _ = write!(output, "cases({}) ", content);
}

/// Convert a verbatim environment
fn convert_verbatim(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    let content = conv.extract_env_raw_content(node);
    output.push_str("```\n");
    output.push_str(content.trim());
    output.push_str("\n```\n");
}

/// Convert an lstlisting environment
fn convert_lstlisting(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    // Parse options using CodeBlockOptions
    let options_str = conv.get_env_optional_arg(node).unwrap_or_default();
    let options = CodeBlockOptions::parse(&options_str);

    // Get Typst language identifier
    let lang = options.get_typst_language();

    let content = conv.extract_env_raw_content(node);

    // If there's a caption, wrap in figure
    if let Some(ref caption) = options.caption {
        output.push_str("\n#figure(\n");
        output.push_str("```");
        output.push_str(lang);
        output.push('\n');
        output.push_str(content.trim());
        output.push_str("\n```,\n");
        let _ = writeln!(output, "  caption: [{}]", caption);
        output.push(')');
        if let Some(ref label) = options.label {
            let _ = write!(output, " <{}>", sanitize_label(label));
        }
        output.push('\n');
    } else {
        output.push_str("\n```");
        output.push_str(lang);
        output.push('\n');
        output.push_str(content.trim());
        output.push_str("\n```\n");
    }
}

/// Convert a minted environment
fn convert_minted(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    // Minted: \begin{minted}[options]{language} ... \end{minted}
    let options_str = conv.get_env_optional_arg(node).unwrap_or_default();
    let options = CodeBlockOptions::parse(&options_str);

    // Get language from required argument
    let lang_raw = conv.get_env_required_arg(node, 0).unwrap_or_default();
    let lang = LANGUAGE_MAP
        .get(lang_raw.as_str())
        .copied()
        .unwrap_or_else(|| lang_raw.to_lowercase().leak());

    let content = conv.extract_env_raw_content(node);

    // If there's a caption, wrap in figure
    if let Some(ref caption) = options.caption {
        output.push_str("\n#figure(\n");
        output.push_str("```");
        output.push_str(lang);
        output.push('\n');
        output.push_str(content.trim());
        output.push_str("\n```,\n");
        let _ = writeln!(output, "  caption: [{}]", caption);
        output.push(')');
        if let Some(ref label) = options.label {
            let _ = write!(output, " <{}>", sanitize_label(label));
        }
        output.push('\n');
    } else {
        output.push_str("\n```");
        output.push_str(lang);
        output.push('\n');
        output.push_str(content.trim());
        output.push_str("\n```\n");
    }
}

/// Convert a tikzpicture environment
fn convert_tikz(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::TikZ);

    // Use the TikZ to CeTZ transpiler
    let tikz_source = node.text().to_string();
    let cetz_code = crate::tikz::convert_tikz_to_cetz(&tikz_source);

    output.push_str("\n// TikZ converted to CeTZ\n");
    output.push_str(&cetz_code);
    output.push('\n');

    conv.state.pop_env();
}

/// Convert a theorem-like environment
fn convert_theorem(
    conv: &mut LatexConverter,
    node: &SyntaxNode,
    env_name: &str,
    output: &mut String,
) {
    let env_ctx = EnvironmentContext::Theorem(env_name.to_string());
    conv.state.push_env(env_ctx);

    // Get theorem info from mapping table, or use defaults
    let (display_name, _style) = if let Some(custom) = conv.state.custom_theorems.get(env_name) {
        (custom.clone(), TheoremStyle::Plain)
    } else if let Some(info) = THEOREM_TYPES.get(env_name) {
        (info.display_name.to_string(), info.style)
    } else {
        // Fallback: capitalize first letter
        let name = env_name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + &env_name[1..];
        (name, TheoremStyle::Plain)
    };

    // Proof doesn't get numbered
    let is_proof = env_name == "proof";

    // Check for optional argument (theorem name/attribution)
    let custom_name = conv.get_env_optional_arg(node);

    if is_proof {
        // Render proof inline with a QED symbol.
        let mut buf = String::new();
        buf.push('\n');
        let _ = write!(buf, "_{}._", display_name);
        if let Some(name) = custom_name {
            let _ = write!(buf, " _({}.)_", name);
        }
        buf.push(' ');
        conv.visit_env_content(node, &mut buf);
        buf.push_str(" #h(1fr) $square.stroked$");
        buf.push_str("\n\n");
        output.push_str(&buf);
        conv.state.pop_env();
        return;
    }

    // For theorem-like blocks, wrap in a referenceable figure.
    let mut label = String::new();
    for child in node.children_with_tokens() {
        if let SyntaxElement::Node(n) = &child {
            if let Some(cmd) = CmdItem::cast(n.clone()) {
                if let Some(name_tok) = cmd.name_tok() {
                    if name_tok.text() == "\\label" {
                        if let Some(lbl) = conv.get_required_arg(&cmd, 0) {
                            label = lbl;
                        }
                    }
                }
            }
        }
    }

    let mut body = String::new();
    if let Some(name) = custom_name {
        let _ = write!(body, "_({}.)_ ", name);
    }
    conv.visit_env_content(node, &mut body);

    output.push_str("\n#figure(kind: \"theorem\", supplement: [");
    output.push_str(&display_name);
    output.push_str("], caption: [])[\n");
    output.push_str(body.trim());
    output.push_str("\n] ");
    if !label.is_empty() {
        let _ = write!(output, "<{}>", sanitize_label(&label));
    }
    output.push_str("\n\n");

    conv.state.pop_env();
}

/// Convert a bibliography environment
fn convert_bibliography(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    conv.state.push_env(EnvironmentContext::Bibliography);

    output.push_str("\n= References\n\n");
    // Render bibliography content as raw text to avoid invalid markup from BibTeX-like entries.
    let raw = conv.extract_env_raw_content(node);
    let trimmed = raw.trim_end();
    let escaped = escape_typst_string(trimmed);
    output.push_str("#raw(block: true, lang: \"text\", \"");
    output.push_str(&escaped);
    output.push_str("\")\n");

    conv.state.pop_env();
}

// convert_thebibliography_content removed; we render raw for stability.

/// Convert a beamer frame
fn convert_frame(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    let title = conv
        .get_env_optional_arg(node)
        .or_else(|| conv.get_env_required_arg(node, 0));

    output.push_str("#slide[\n");

    if let Some(t) = title {
        let _ = write!(output, "  == {}\n\n", t);
    }

    conv.visit_env_content(node, output);

    output.push_str("\n]\n");
}

/// Convert a subfigure
fn convert_subfigure(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    let width = conv
        .get_env_optional_arg(node)
        .unwrap_or("0.5\\linewidth".to_string());
    let width_typst = convert_dimension(&width);

    let _ = writeln!(output, "#box(width: {})[", width_typst);
    conv.visit_env_content(node, output);
    output.push_str("\n]\n");
}

/// Convert an algorithm environment
fn convert_algorithm(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    output.push_str("#block(width: 100%, stroke: 1pt, inset: 10pt)[\n");
    output.push_str("  #text(weight: \"bold\")[Algorithm]\n\n");

    // Process as code-like content
    let content = conv.extract_env_raw_content(node);
    let trimmed = content.trim_end();
    let escaped = escape_typst_string(trimmed);
    output.push_str("  #raw(block: true, lang: \"text\", \"");
    output.push_str(&escaped);
    output.push_str("\")\n");

    output.push_str("]\n");
}

/// Convert a gameproof environment (cryptocode) as raw text to keep it valid.
fn convert_gameproof(conv: &mut LatexConverter, node: &SyntaxNode, output: &mut String) {
    let content = conv.extract_env_raw_content(node);
    let trimmed = content.trim_end();
    let escaped = escape_typst_string(trimmed);
    output.push_str("\n#block(width: 100%, stroke: 1pt, inset: 10pt)[\n");
    output.push_str("  #text(weight: \"bold\")[Game]\n\n");
    output.push_str("  #raw(block: true, lang: \"text\", \"");
    output.push_str(&escaped);
    output.push_str("\")\n");
    output.push_str("]\n");
}

// =============================================================================
// Helper functions
// =============================================================================

/// Get the column specification from a tabular environment
/// The col spec is in the first curly arg after the env name: \begin{tabular}{lccc}
fn get_tabular_col_spec(node: &SyntaxNode) -> Option<String> {
    // Look for ItemBegin, then find the column specification argument
    for child in node.children() {
        if child.kind() == SyntaxKind::ItemBegin {
            // In ItemBegin, look for ClauseArgument with curly braces
            for begin_child in child.children() {
                if begin_child.kind() == SyntaxKind::ClauseArgument {
                    // Check if it's a curly (required) argument
                    let has_curly = begin_child
                        .children()
                        .any(|c| c.kind() == SyntaxKind::ItemCurly);
                    if has_curly {
                        // Extract the content
                        let mut content = String::new();
                        for arg_child in begin_child.children_with_tokens() {
                            match arg_child.kind() {
                                SyntaxKind::TokenLBrace
                                | SyntaxKind::TokenRBrace
                                | SyntaxKind::TokenLBracket
                                | SyntaxKind::TokenRBracket => continue,
                                SyntaxKind::ItemCurly => {
                                    // Extract inner content
                                    if let SyntaxElement::Node(n) = arg_child {
                                        for inner in n.children_with_tokens() {
                                            match inner.kind() {
                                                SyntaxKind::TokenLBrace
                                                | SyntaxKind::TokenRBrace => continue,
                                                _ => {
                                                    if let SyntaxElement::Token(t) = inner {
                                                        content.push_str(t.text());
                                                    } else if let SyntaxElement::Node(n) = inner {
                                                        content.push_str(&n.text().to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    if let SyntaxElement::Token(t) = arg_child {
                                        content.push_str(t.text());
                                    }
                                }
                            }
                        }
                        let trimmed = content.trim().to_string();
                        if !trimmed.is_empty() {
                            return Some(trimmed);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Parse column specification from LaTeX format (e.g., "l|ccc" -> ["l", "c", "c", "c"])
fn parse_column_spec(spec: &str) -> Vec<String> {
    let mut columns = Vec::new();
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let c = chars[i];
        match c {
            'l' | 'c' | 'r' => {
                columns.push(c.to_string());
                i += 1;
            }
            'p' | 'm' | 'b' | 'X' | 'S' => {
                i += 1;
                i = skip_ws(&chars, i);
                if i < chars.len() && chars[i] == '{' {
                    i = skip_braced_group(&chars, i);
                }
                columns.push("l".to_string());
            }
            '*' => {
                i += 1;
                i = skip_ws(&chars, i);
                let mut count = 1usize;
                if i < chars.len() && chars[i] == '{' {
                    if let Some((count_str, next)) = parse_braced_content(&chars, i) {
                        count = count_str.trim().parse().unwrap_or(1);
                        i = next;
                    } else {
                        i = chars.len();
                    }
                }
                i = skip_ws(&chars, i);
                if i < chars.len() && chars[i] == '{' {
                    if let Some((spec_str, next)) = parse_braced_content(&chars, i) {
                        let inner_cols = parse_column_spec(&spec_str);
                        for _ in 0..count {
                            columns.extend(inner_cols.clone());
                        }
                        i = next;
                    } else {
                        i = chars.len();
                    }
                }
            }
            '>' | '<' => {
                i += 1;
                i = skip_ws(&chars, i);
                if i < chars.len() && chars[i] == '{' {
                    i = skip_braced_group(&chars, i);
                }
            }
            '@' | '!' => {
                i += 1;
                i = skip_ws(&chars, i);
                if i < chars.len() && chars[i] == '{' {
                    i = skip_braced_group(&chars, i);
                }
            }
            '|' => {
                i += 1;
            }
            _ if c.is_ascii_whitespace() => {
                i += 1;
            }
            _ if c.is_ascii_alphabetic() => {
                i += 1;
                i = skip_ws(&chars, i);
                if i < chars.len() && chars[i] == '{' {
                    i = skip_braced_group(&chars, i);
                }
                columns.push("l".to_string());
            }
            _ => {
                i += 1;
            }
        }
    }

    if columns.is_empty() {
        columns.push("l".to_string());
    }

    columns
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn skip_braced_group(chars: &[char], start: usize) -> usize {
    let mut depth = 0i32;
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    chars.len()
}

fn parse_braced_content(chars: &[char], start: usize) -> Option<(String, usize)> {
    if start >= chars.len() || chars[start] != '{' {
        return None;
    }
    let mut depth = 0i32;
    let mut out = String::new();
    let mut i = start;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' {
            depth += 1;
            if depth > 1 {
                out.push(ch);
            }
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((out, i + 1));
            }
            out.push(ch);
        } else if depth >= 1 {
            out.push(ch);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_column_spec;

    #[test]
    fn test_parse_column_spec_with_modifiers() {
        let cols = parse_column_spec(r">{\raggedright\arraybackslash}p{2cm}c");
        assert_eq!(cols, vec!["l".to_string(), "c".to_string()]);
    }

    #[test]
    fn test_parse_column_spec_with_repeat_and_custom() {
        let cols = parse_column_spec(r"l|*{2}{>{\centering}m{1cm}}r");
        assert_eq!(
            cols,
            vec![
                "l".to_string(),
                "l".to_string(),
                "l".to_string(),
                "r".to_string()
            ]
        );
    }

    #[test]
    fn test_parse_column_spec_custom_types() {
        let cols = parse_column_spec(r"S D{.}{.}{-1} c");
        assert_eq!(
            cols,
            vec!["l".to_string(), "l".to_string(), "c".to_string()]
        );
    }
}

/// Convert a LaTeX dimension to Typst
fn convert_dimension(dim: &str) -> String {
    let dim = dim.trim();

    if dim.contains("\\linewidth") || dim.contains("\\textwidth") {
        if let Some(mult) = dim
            .strip_suffix("\\linewidth")
            .or(dim.strip_suffix("\\textwidth"))
        {
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

    dim.to_string()
}
