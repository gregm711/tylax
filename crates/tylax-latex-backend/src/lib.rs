//! IR to LaTeX backend.

use tylax_ir::{
    Alignment, Block, Document, EnvironmentBlock, Figure, FigureContent, Grid, Image, Inline,
    ListKind, MathBlock, Table, TableCell,
};

#[derive(Debug, Clone)]
pub struct LatexRenderOptions {
    pub full_document: bool,
    pub number_equations: bool,
    pub two_column: bool,
    pub inline_wide_tables: bool,
    pub force_here: bool,
    pub table_grid: bool,
    pub table_style: TableStyle,
    pub table_caption_position: TableCaptionPosition,
    pub bibliography_style_default: Option<String>,
    pub cite_command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableStyle {
    Plain,
    Grid,
    Booktabs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCaptionPosition {
    Top,
    Bottom,
}

impl Default for LatexRenderOptions {
    fn default() -> Self {
        Self {
            full_document: false,
            number_equations: false,
            two_column: false,
            inline_wide_tables: false,
            force_here: false,
            table_grid: false,
            table_style: TableStyle::Plain,
            table_caption_position: TableCaptionPosition::Bottom,
            bibliography_style_default: None,
            cite_command: None,
        }
    }
}

pub fn render_document(doc: &Document, options: LatexRenderOptions) -> String {
    let mut out = String::new();
    if options.full_document {
        out.push_str("\\documentclass{article}\n");
        out.push_str("\\usepackage{amsmath,amssymb}\n");
        out.push_str("\\usepackage{graphicx}\n");
        out.push_str("\\usepackage{hyperref}\n");
        out.push_str("\\usepackage[table]{xcolor}\n");
        out.push_str("\\usepackage{enumitem}\n");
        out.push_str("\\usepackage{multirow}\n");
        out.push_str("\\usepackage{multicol}\n");
        out.push_str("\\usepackage{array}\n");
        if options.table_style == TableStyle::Booktabs {
            out.push_str("\\usepackage{booktabs}\n");
        }
        if options.inline_wide_tables {
            out.push_str("\\usepackage{caption}\n");
        }
        out.push_str("\\providecommand{\\textsubscript}[1]{$_{\\text{#1}}$}\n");
        out.push_str("\\begin{document}\n\n");
    }

    let mut first = true;
    let mut idx = 0usize;
    while idx < doc.blocks.len() {
        let mut rendered: Option<String> = None;
        let mut consumed = 1usize;

        if let Some(rendered_refs) = render_references_block(&doc.blocks, idx, &options) {
            rendered = Some(rendered_refs);
            consumed = 2;
        } else if let Some(rendered_heading) = render_heading_with_label(&doc.blocks, idx, &options)
        {
            rendered = Some(rendered_heading);
            consumed = 2;
        } else if let Some(rendered_env) = render_environment_with_label(&doc.blocks, idx, &options)
        {
            rendered = Some(rendered_env);
            consumed = 2;
        } else if let Some(rendered_table) = render_table_with_label(&doc.blocks, idx, &options) {
            rendered = Some(rendered_table);
            consumed = 2;
        }

        let chunk = rendered.unwrap_or_else(|| render_block(&doc.blocks[idx], &options));
        if !chunk.trim().is_empty() {
            if !first {
                out.push_str("\n\n");
            }
            out.push_str(&chunk);
            first = false;
        }
        idx += consumed;
    }

    if options.full_document {
        out.push_str("\n\\end{document}\n");
    }

    out
}

fn render_heading_with_label(
    blocks: &[Block],
    idx: usize,
    options: &LatexRenderOptions,
) -> Option<String> {
    let heading = blocks.get(idx)?;
    let Block::Heading { .. } = heading else {
        return None;
    };
    let label = blocks
        .get(idx + 1)
        .and_then(extract_label_from_paragraph)?;
    let mut out = render_block(heading, options);
    out.push_str("\n\\label{");
    out.push_str(&escape_latex(&label));
    out.push('}');
    Some(out)
}

fn render_environment_with_label(
    blocks: &[Block],
    idx: usize,
    options: &LatexRenderOptions,
) -> Option<String> {
    let env_block = blocks.get(idx)?;
    let Block::Environment(env) = env_block else {
        return None;
    };
    let label = blocks
        .get(idx + 1)
        .and_then(extract_label_from_paragraph)?;
    Some(render_environment(env, options, Some(&label)))
}

fn render_table_with_label(
    blocks: &[Block],
    idx: usize,
    options: &LatexRenderOptions,
) -> Option<String> {
    let table_block = blocks.get(idx)?;
    let Block::Table(table) = table_block else {
        return None;
    };
    let label = blocks
        .get(idx + 1)
        .and_then(extract_label_from_paragraph)?;
    Some(render_table_block(table, options, Some(&label)))
}

fn extract_label_from_paragraph(block: &Block) -> Option<String> {
    let Block::Paragraph(inlines) = block else {
        return None;
    };
    let mut label: Option<String> = None;
    for inline in inlines {
        match inline {
            Inline::Label(value) => {
                if label.is_some() {
                    return None;
                }
                label = Some(value.clone());
            }
            Inline::Text(text) => {
                if !text.trim().is_empty() {
                    return None;
                }
            }
            Inline::LineBreak => {}
            _ => return None,
        }
    }
    label
}

fn render_references_block(
    blocks: &[Block],
    idx: usize,
    options: &LatexRenderOptions,
) -> Option<String> {
    let heading = blocks.get(idx)?;
    let Block::Heading { content, .. } = heading else {
        return None;
    };
    let title = normalize_inline_whitespace(&render_inlines(content, options));
    if !is_references_title(&title) {
        return None;
    }
    let next = blocks.get(idx + 1)?;
    match next {
        Block::Paragraph(inlines) => {
            let entries = split_reference_entries(inlines);
            if entries.is_empty() {
                return None;
            }

            let mut out = String::new();
            out.push_str("\\begin{thebibliography}{99}\n");
            for (i, entry) in entries.iter().enumerate() {
                let rendered = normalize_inline_whitespace(&render_inlines(entry, options));
                if rendered.is_empty() {
                    continue;
                }
                let (label, body) = strip_reference_prefix(&rendered);
                let key = label.unwrap_or_else(|| format!("ref{}", i + 1));
                out.push_str(&format!("\\bibitem{{{}}} {}\n", key, body));
            }
            out.push_str("\\end{thebibliography}");
            Some(out)
        }
        Block::Bibliography { file, style } => {
            let style = style
                .as_deref()
                .or_else(|| options.bibliography_style_default.as_deref());
            Some(render_bibliography(file, style))
        }
        Block::Environment(env) if env.name == "thebibliography" => {
            Some(render_environment(env, options, None))
        }
        _ => None,
    }
}

fn is_references_title(title: &str) -> bool {
    let lowered = title.trim().to_lowercase();
    lowered == "references" || lowered == "bibliography"
}

fn split_reference_entries(inlines: &[Inline]) -> Vec<Vec<Inline>> {
    let mut entries: Vec<Vec<Inline>> = Vec::new();
    let mut current: Vec<Inline> = Vec::new();
    for inline in inlines {
        if matches!(inline, Inline::LineBreak) {
            if !current.is_empty() {
                entries.push(current);
                current = Vec::new();
            }
            continue;
        }
        current.push(inline.clone());
    }
    if !current.is_empty() {
        entries.push(current);
    }
    entries
}

fn strip_reference_prefix(text: &str) -> (Option<String>, String) {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return (None, trimmed.to_string());
    };
    let mut digits = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            chars.next();
        } else if ch.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    if chars.peek().copied() == Some(']') {
        chars.next();
        let remainder: String = chars.collect();
        let body = remainder.trim_start().to_string();
        if !digits.is_empty() {
            return (Some(format!("ref{}", digits)), body);
        }
        return (None, body);
    }
    (None, trimmed.to_string())
}

fn render_block(block: &Block, options: &LatexRenderOptions) -> String {
    match block {
        Block::Paragraph(inlines) => normalize_inline_whitespace(&render_inlines(inlines, options)),
        Block::VSpace(size) => render_vspace(size),
        Block::Heading {
            level,
            content,
            numbered,
        } => {
            let cmd = match *level {
                1 => "\\section",
                2 => "\\subsection",
                3 => "\\subsubsection",
                4 => "\\paragraph",
                _ => "\\section",
            };
            let cmd = if *numbered {
                cmd.to_string()
            } else {
                format!("{}*", cmd)
            };
            format!(
                "{}{{{}}}",
                cmd,
                normalize_inline_whitespace(&render_inlines(content, options))
            )
        }
        Block::List { kind, items } => {
            let env = match kind {
                ListKind::Unordered => "itemize",
                ListKind::Ordered => "enumerate",
            };
            let mut out = String::new();
            out.push_str(&format!("\\begin{{{}}}\n", env));
            for item in items {
                out.push_str("  \\item ");
                out.push_str(&render_blocks_inline(item, options));
                out.push('\n');
            }
            out.push_str(&format!("\\end{{{}}}", env));
            out
        }
        Block::MathBlock(math) => render_math_block(math, options),
        Block::CodeBlock(content) => format!("\\begin{{verbatim}}\n{}\n\\end{{verbatim}}", content),
        Block::Quote(blocks) => {
            let mut out = String::new();
            out.push_str("\\begin{quote}\n");
            out.push_str(&render_blocks_inline(blocks, options));
            out.push_str("\n\\end{quote}");
            out
        }
        Block::Align { alignment, blocks } => {
            let env = match alignment {
                Alignment::Left => "flushleft",
                Alignment::Right => "flushright",
                Alignment::Center => "center",
            };
            let mut out = String::new();
            out.push_str(&format!("\\begin{{{}}}\n", env));
            out.push_str(&render_blocks_inline(blocks, options));
            out.push_str(&format!("\n\\end{{{}}}", env));
            out
        }
        Block::Table(table) => render_table_block(table, options, None),
        Block::Figure(figure) => render_figure(figure, options),
        Block::Environment(env) => render_environment(env, options, None),
        Block::Bibliography { file, style } => {
            let style = style
                .as_deref()
                .or_else(|| options.bibliography_style_default.as_deref());
            render_bibliography(file, style)
        }
        Block::Outline { title } => render_outline(title.as_deref(), options),
        Block::Box(b) => render_box(&b.blocks, options),
        Block::Block(b) => render_block_wrapper(&b.blocks, options),
        Block::Columns(columns) => render_columns(columns, options),
        Block::Grid(grid) => render_grid(grid, options),
    }
}

fn render_blocks_inline(blocks: &[Block], options: &LatexRenderOptions) -> String {
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            let prev = &blocks[i - 1];
            if matches!(prev, Block::VSpace(_)) || matches!(block, Block::VSpace(_)) {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str(&render_block(block, options));
    }
    out
}

fn render_math_block(math: &MathBlock, options: &LatexRenderOptions) -> String {
    let raw = math.content.trim();
    let content = convert_math_content(raw);
    let content = content.trim();
    let has_alignment = raw.contains('&');
    let has_line_break = raw.contains("\\\\");
    let mut out = String::new();
    if has_alignment || has_line_break {
        let env = if has_alignment {
            if options.number_equations {
                "align"
            } else {
                "align*"
            }
        } else if options.number_equations {
            "gather"
        } else {
            "gather*"
        };
        out.push_str(&format!("\\begin{{{}}}\n", env));
        out.push_str(content);
        if let Some(label) = &math.label {
            out.push_str("\n\\label{");
            out.push_str(&escape_latex(label));
            out.push('}');
        }
        out.push_str(&format!("\n\\end{{{}}}", env));
        return out;
    }
    if options.number_equations {
        out.push_str("\\begin{equation}\n");
        out.push_str(content);
        if let Some(label) = &math.label {
            out.push_str("\n\\label{");
            out.push_str(&escape_latex(label));
            out.push('}');
        }
        out.push_str("\n\\end{equation}");
    } else {
        out.push_str("\\[\n");
        out.push_str(content);
        out.push_str("\n\\]");
    }
    out
}

fn convert_math_content(input: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < input.len() {
        if let Some(paren_idx) = match_call_at(input, i, "text") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_text_call(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "upright") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_upright_call(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "cases") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_cases(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "mat") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_mat(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "frac") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_frac(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "sqrt") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_sqrt(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "root") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_root(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "binom") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_binom(&args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "sin") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("sin", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "cos") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("cos", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "tan") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("tan", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "log") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("log", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "ln") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("ln", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "exp") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("exp", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "lim") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call("lim", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "max") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call_multi("max", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "min") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call_multi("min", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "sup") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call_multi("sup", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "inf") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_call_multi("inf", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "argmax") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_star_call("argmax", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "argmin") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_operator_star_call("argmin", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "abs") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_wrapped(
                    &args,
                    "\\left\\lvert ",
                    " \\right\\rvert",
                ));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "norm") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_wrapped(
                    &args,
                    "\\left\\lVert ",
                    " \\right\\rVert",
                ));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "ceil") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_wrapped(
                    &args,
                    "\\left\\lceil ",
                    " \\right\\rceil",
                ));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "floor") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_wrapped(
                    &args,
                    "\\left\\lfloor ",
                    " \\right\\rfloor",
                ));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "vec") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("vec", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "hat") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("hat", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "tilde") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("tilde", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "bar") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("bar", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "dot") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("dot", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "ddot") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("ddot", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "overline") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("overline", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "underline") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_unary_command("underline", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "bb") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_font_command("mathbb", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "cal") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_font_command("mathcal", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "frak") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_font_command("mathfrak", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some(paren_idx) = match_call_at(input, i, "bold") {
            if let Some((args, end_idx)) = extract_paren_content(input, paren_idx) {
                out.push_str(&convert_font_command("mathbf", &args));
                i = end_idx;
                continue;
            }
        }
        if let Some((latex, len)) = match_sequence_at(input, i) {
            out.push_str(latex);
            i += len;
            continue;
        }
        if let Some((latex, len)) = match_dotted_symbol_at(input, i) {
            out.push_str(latex);
            i += len;
            continue;
        }
        if let Some((latex, len)) = match_symbol_at(input, i) {
            out.push_str(latex);
            i += len;
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn match_call_at(input: &str, idx: usize, name: &str) -> Option<usize> {
    if !input[idx..].starts_with(name) {
        return None;
    }
    if idx > 0 {
        if let Some(prev) = input[..idx].chars().rev().find(|c| !c.is_whitespace()) {
            if prev == '\\' || prev.is_ascii_alphanumeric() || prev == '_' {
                return None;
            }
        }
    }
    let mut j = idx + name.len();
    while j < input.len() {
        let ch = input[j..].chars().next().unwrap();
        if ch.is_whitespace() {
            j += ch.len_utf8();
        } else {
            break;
        }
    }
    if j < input.len() && input[j..].starts_with('(') {
        Some(j)
    } else {
        None
    }
}

fn extract_paren_content(input: &str, start_paren: usize) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = start_paren;
    let mut content_start = None;
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        if content_start.is_none() {
            if ch != '(' {
                return None;
            }
            depth = 1;
            i += ch.len_utf8();
            content_start = Some(i);
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += ch.len_utf8();
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let start = content_start.unwrap_or(i);
                    let content = input[start..i].to_string();
                    i += ch.len_utf8();
                    return Some((content, i));
                }
            }
            _ => {}
        }
        i += ch.len_utf8();
    }
    None
}

fn split_top_level(input: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth_paren = 0usize;
    let mut depth_brack = 0usize;
    let mut depth_brace = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        let ch = input[i..].chars().next().unwrap();
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            i += ch.len_utf8();
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            '[' => depth_brack += 1,
            ']' => depth_brack = depth_brack.saturating_sub(1),
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            _ => {}
        }
        if ch == delim
            && depth_paren == 0
            && depth_brack == 0
            && depth_brace == 0
        {
            parts.push(input[start..i].to_string());
            i += ch.len_utf8();
            start = i;
            continue;
        }
        i += ch.len_utf8();
    }
    if start <= input.len() {
        parts.push(input[start..].to_string());
    }
    parts
}

fn convert_cases(args: &str) -> String {
    let entries = split_top_level(args, ',');
    let mut rows = Vec::new();
    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_cases_named_arg(trimmed) {
            continue;
        }
        let converted = convert_math_content(trimmed);
        let (expr, cond) = split_case_condition(&converted);
        let row = if let Some(cond) = cond {
            format!("{} & {}", expr, cond)
        } else {
            expr
        };
        rows.push(row);
    }
    let mut out = String::new();
    out.push_str("\\begin{cases}");
    if !rows.is_empty() {
        out.push_str(&rows.join(" \\\\ "));
    }
    out.push_str("\\end{cases}");
    out
}

fn is_cases_named_arg(entry: &str) -> bool {
    let lower = entry.trim_start().to_lowercase();
    lower.starts_with("delim:")
        || lower.starts_with("reverse:")
        || lower.starts_with("gap:")
        || lower.starts_with("row-gap:")
        || lower.starts_with("column-gap:")
}

fn split_case_condition(entry: &str) -> (String, Option<String>) {
    if let Some(idx) = entry.find("\"if\"") {
        let left = entry[..idx].trim().to_string();
        let right = entry[idx + 4..].trim();
        let cond = if right.is_empty() {
            "\\text{if}".to_string()
        } else {
            format!("\\text{{if}} {}", right)
        };
        return (left, Some(cond));
    }
    if let Some(idx) = entry.find("\"else\"") {
        let left = entry[..idx].trim().to_string();
        let right = entry[idx + 6..].trim();
        let cond = if right.is_empty() {
            "\\text{else}".to_string()
        } else {
            format!("\\text{{else}} {}", right)
        };
        return (left, Some(cond));
    }
    (entry.trim().to_string(), None)
}

fn convert_mat(args: &str) -> String {
    let rows_raw = split_top_level(args, ';');
    let mut rows = Vec::new();
    let mut delim: Option<String> = None;
    for row in rows_raw {
        let cols_raw = split_top_level(&row, ',');
        let mut cols = Vec::new();
        for col in cols_raw {
            let trimmed = col.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(value) = parse_named_arg(trimmed, "delim") {
                if delim.is_none() {
                    delim = Some(value);
                }
                continue;
            }
            if is_mat_named_arg(trimmed) {
                continue;
            }
            cols.push(convert_math_content(trimmed));
        }
        if !cols.is_empty() {
            rows.push(cols.join(" & "));
        }
    }
    let env = mat_env_from_delim(delim.as_deref());
    let mut out = String::new();
    out.push_str(&format!("\\begin{{{}}}", env));
    if !rows.is_empty() {
        out.push_str(&rows.join(" \\\\ "));
    }
    out.push_str(&format!("\\end{{{}}}", env));
    out
}

fn convert_frac(args: &str) -> String {
    let parts = positional_args(args);
    if parts.len() >= 2 {
        return format!(
            "\\frac{{{}}}{{{}}}",
            convert_math_content(&parts[0]),
            convert_math_content(&parts[1])
        );
    }
    format!(
        "\\operatorname{{frac}}\\left({}\\right)",
        convert_math_content(args)
    )
}

fn convert_sqrt(args: &str) -> String {
    if let Some(arg) = first_positional_arg(args) {
        return format!("\\sqrt{{{}}}", convert_math_content(&arg));
    }
    "\\sqrt{}".to_string()
}

fn convert_root(args: &str) -> String {
    let parts = positional_args(args);
    if parts.len() >= 2 {
        return format!(
            "\\sqrt[{}]{{{}}}",
            convert_math_content(&parts[0]),
            convert_math_content(&parts[1])
        );
    }
    if let Some(arg) = parts.get(0) {
        return format!("\\sqrt{{{}}}", convert_math_content(arg));
    }
    "\\sqrt{}".to_string()
}

fn convert_binom(args: &str) -> String {
    let parts = positional_args(args);
    if parts.len() >= 2 {
        return format!(
            "\\binom{{{}}}{{{}}}",
            convert_math_content(&parts[0]),
            convert_math_content(&parts[1])
        );
    }
    format!(
        "\\operatorname{{binom}}\\left({}\\right)",
        convert_math_content(args)
    )
}

fn convert_operator_call(op: &str, args: &str) -> String {
    let arg = first_positional_arg(args).unwrap_or_default();
    if arg.is_empty() {
        return format!("\\{}", op);
    }
    format!("\\{}\\left({}\\right)", op, convert_math_content(&arg))
}

fn convert_operator_call_multi(op: &str, args: &str) -> String {
    let parts = positional_args(args);
    if parts.is_empty() {
        return format!("\\{}", op);
    }
    let rendered = parts
        .into_iter()
        .map(|part| convert_math_content(&part))
        .collect::<Vec<_>>()
        .join(", ");
    format!("\\{}\\left({}\\right)", op, rendered)
}

fn convert_operator_star_call(op: &str, args: &str) -> String {
    let arg = first_positional_arg(args).unwrap_or_default();
    if arg.is_empty() {
        return format!("\\operatorname*{{{}}}", op);
    }
    format!(
        "\\operatorname*{{{}}}\\left({}\\right)",
        op,
        convert_math_content(&arg)
    )
}

fn convert_wrapped(args: &str, left: &str, right: &str) -> String {
    if let Some(arg) = first_positional_arg(args) {
        return format!("{}{}{}", left, convert_math_content(&arg), right);
    }
    format!("{}{}", left, right)
}

fn convert_unary_command(cmd: &str, args: &str) -> String {
    let arg = first_positional_arg(args).unwrap_or_default();
    format!("\\{}{{{}}}", cmd, convert_math_content(&arg))
}

fn convert_text_call(args: &str) -> String {
    let arg = first_positional_arg(args).unwrap_or_default();
    let text = strip_quotes(&arg);
    format!("\\text{{{}}}", escape_latex(&text))
}

fn convert_upright_call(args: &str) -> String {
    let arg = first_positional_arg(args).unwrap_or_default();
    let text = strip_quotes(&arg);
    format!("\\mathrm{{{}}}", escape_latex(&text))
}

fn convert_font_command(cmd: &str, args: &str) -> String {
    let arg = first_positional_arg(args).unwrap_or_default();
    let arg = strip_quotes(&arg);
    format!("\\{}{{{}}}", cmd, convert_math_content(&arg))
}

fn positional_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in split_top_level(args, ',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_named_arg_like(trimmed) {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}

fn first_positional_arg(args: &str) -> Option<String> {
    positional_args(args).into_iter().next()
}

fn is_named_arg_like(input: &str) -> bool {
    let mut parts = input.splitn(2, ':');
    let Some(head) = parts.next() else {
        return false;
    };
    let Some(_) = parts.next() else {
        return false;
    };
    let head = head.trim();
    if head.is_empty() {
        return false;
    }
    head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn strip_quotes(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

fn match_token_at(input: &str, idx: usize, token: &str) -> bool {
    if !input[idx..].starts_with(token) {
        return false;
    }
    if idx > 0 {
        if let Some(prev) = input[..idx].chars().rev().next() {
            if prev == '\\' || prev.is_ascii_alphanumeric() {
                return false;
            }
        }
    }
    let end = idx + token.len();
    if end < input.len() {
        if let Some(next) = input[end..].chars().next() {
            if next.is_ascii_alphanumeric() {
                return false;
            }
        }
    }
    true
}

fn match_sequence_at(input: &str, idx: usize) -> Option<(&'static str, usize)> {
    const MAP: &[(&str, &str)] = &[
        ("<=>", "\\Leftrightarrow"),
        ("<->", "\\leftrightarrow"),
        ("=>", "\\Rightarrow"),
        ("<-", "\\leftarrow"),
        ("->", "\\to"),
        ("<=", "\\le"),
        (">=", "\\ge"),
        ("!=", "\\ne"),
        ("...", "\\ldots"),
    ];
    for (token, latex) in MAP {
        if input[idx..].starts_with(token) {
            return Some((latex, token.len()));
        }
    }
    None
}

fn match_symbol_at(input: &str, idx: usize) -> Option<(&'static str, usize)> {
    const MAP: &[(&str, &str)] = &[
        ("oo", "\\infty"),
        ("dif", "\\mathrm{d}"),
        ("varepsilon", "\\varepsilon"),
        ("vartheta", "\\vartheta"),
        ("varsigma", "\\varsigma"),
        ("varphi", "\\varphi"),
        ("varrho", "\\varrho"),
        ("varpi", "\\varpi"),
        ("epsilon", "\\epsilon"),
        ("theta", "\\theta"),
        ("sigma", "\\sigma"),
        ("phi", "\\phi"),
        ("rho", "\\rho"),
        ("pi", "\\pi"),
        ("alpha", "\\alpha"),
        ("beta", "\\beta"),
        ("gamma", "\\gamma"),
        ("delta", "\\delta"),
        ("zeta", "\\zeta"),
        ("eta", "\\eta"),
        ("iota", "\\iota"),
        ("kappa", "\\kappa"),
        ("lambda", "\\lambda"),
        ("mu", "\\mu"),
        ("nu", "\\nu"),
        ("xi", "\\xi"),
        ("tau", "\\tau"),
        ("upsilon", "\\upsilon"),
        ("chi", "\\chi"),
        ("psi", "\\psi"),
        ("omega", "\\omega"),
        ("Gamma", "\\Gamma"),
        ("Delta", "\\Delta"),
        ("Theta", "\\Theta"),
        ("Lambda", "\\Lambda"),
        ("Xi", "\\Xi"),
        ("Pi", "\\Pi"),
        ("Sigma", "\\Sigma"),
        ("Upsilon", "\\Upsilon"),
        ("Phi", "\\Phi"),
        ("Psi", "\\Psi"),
        ("Omega", "\\Omega"),
        ("argmax", "\\operatorname*{argmax}"),
        ("argmin", "\\operatorname*{argmin}"),
        ("supseteq", "\\supseteq"),
        ("subseteq", "\\subseteq"),
        ("supset", "\\supset"),
        ("subset", "\\subset"),
        ("notin", "\\notin"),
        ("infty", "\\infty"),
        ("forall", "\\forall"),
        ("exists", "\\exists"),
        ("partial", "\\partial"),
        ("nabla", "\\nabla"),
        ("implies", "\\implies"),
        ("iff", "\\iff"),
        ("approx", "\\approx"),
        ("sim", "\\sim"),
        ("integral", "\\int"),
        ("prod", "\\prod"),
        ("sum", "\\sum"),
        ("lim", "\\lim"),
        ("max", "\\max"),
        ("min", "\\min"),
        ("sup", "\\sup"),
        ("inf", "\\inf"),
        ("int", "\\int"),
        ("in", "\\in"),
        ("RR", "\\mathbb{R}"),
        ("NN", "\\mathbb{N}"),
        ("ZZ", "\\mathbb{Z}"),
        ("QQ", "\\mathbb{Q}"),
        ("CC", "\\mathbb{C}"),
        ("HH", "\\mathbb{H}"),
        ("FF", "\\mathbb{F}"),
        ("EE", "\\mathbb{E}"),
        ("PP", "\\mathbb{P}"),
    ];
    for (token, latex) in MAP {
        if match_token_at(input, idx, token) {
            return Some((latex, token.len()));
        }
    }
    None
}

fn match_dotted_symbol_at(input: &str, idx: usize) -> Option<(&'static str, usize)> {
    const MAP: &[(&str, &str)] = &[
        ("square.stroked", "\\square"),
        ("sym.square.stroked", "\\square"),
    ];
    for (token, latex) in MAP {
        if !input[idx..].starts_with(token) {
            continue;
        }
        if idx > 0 {
            if let Some(prev) = input[..idx].chars().rev().next() {
                if prev.is_ascii_alphanumeric() || prev == '_' || prev == '.' {
                    continue;
                }
            }
        }
        let end = idx + token.len();
        if end < input.len() {
            if let Some(next) = input[end..].chars().next() {
                if next.is_ascii_alphanumeric() || next == '_' || next == '.' {
                    continue;
                }
            }
        }
        return Some((latex, token.len()));
    }
    None
}

fn parse_named_arg(input: &str, name: &str) -> Option<String> {
    let trimmed = input.trim();
    let prefix = format!("{}:", name);
    if trimmed.starts_with(&prefix) {
        let value = trimmed[prefix.len()..].trim();
        return Some(value.trim_matches('"').to_string());
    }
    None
}

fn is_mat_named_arg(entry: &str) -> bool {
    let lower = entry.trim_start().to_lowercase();
    lower.starts_with("delim:")
        || lower.starts_with("augment:")
        || lower.starts_with("align:")
        || lower.starts_with("gap:")
        || lower.starts_with("row-gap:")
        || lower.starts_with("column-gap:")
}

fn mat_env_from_delim(delim: Option<&str>) -> &'static str {
    let Some(raw) = delim else {
        return "pmatrix";
    };
    let lower = raw.trim().trim_matches('"').to_lowercase();
    if lower.contains("none") {
        return "matrix";
    }
    if lower.contains('[') || lower.contains("bracket") {
        return "bmatrix";
    }
    if lower.contains('{') || lower.contains("brace") {
        return "Bmatrix";
    }
    if lower.contains("||") || lower.contains('‖') || lower.contains("double") {
        return "Vmatrix";
    }
    if lower.contains('|') || lower.contains("bar") {
        return "vmatrix";
    }
    if lower.contains('(') || lower.contains("paren") {
        return "pmatrix";
    }
    "pmatrix"
}

fn render_environment(
    env: &EnvironmentBlock,
    options: &LatexRenderOptions,
    label: Option<&str>,
) -> String {
    let name = sanitize_env_name(&env.name);
    let mut out = String::new();
    if name == "proof" {
        out.push_str("\\begin{proof}\n");
    } else if let Some(title) = &env.title {
        out.push_str(&format!(
            "\\begin{{{}}}[{}]\n",
            name,
            normalize_inline_whitespace(&render_inlines(title, options))
        ));
    } else {
        out.push_str(&format!("\\begin{{{}}}\n", name));
    }
    out.push_str(&render_blocks_inline(&env.blocks, options));
    if let Some(label) = label {
        out.push_str("\n\\label{");
        out.push_str(&escape_latex(label));
        out.push('}');
    }
    out.push_str(&format!("\n\\end{{{}}}", name));
    out
}

fn sanitize_env_name(name: &str) -> String {
    let lowered = name.trim().to_lowercase();
    if lowered.is_empty() {
        return "theorem".to_string();
    }
    lowered
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
}

fn render_inlines(inlines: &[Inline], options: &LatexRenderOptions) -> String {
    let mut out = String::new();
    let mut last_was_linebreak = false;
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&escape_latex(text)),
            Inline::Strong(inner) => {
                out.push_str("\\textbf{");
                out.push_str(&render_inlines(inner, options));
                out.push('}');
            }
            Inline::Emph(inner) => {
                out.push_str("\\textit{");
                out.push_str(&render_inlines(inner, options));
                out.push('}');
            }
            Inline::Code(code) => {
                out.push_str("\\texttt{");
                out.push_str(&escape_latex(code));
                out.push('}');
            }
            Inline::Math(content) => {
                out.push('$');
                out.push_str(&convert_math_content(content));
                out.push('$');
            }
            Inline::Link { text, url } => {
                out.push_str("\\href{");
                out.push_str(&escape_latex(url));
                out.push_str("}{");
                out.push_str(&render_inlines(text, options));
                out.push('}');
            }
            Inline::Ref(label) => {
                if is_equation_label(label) {
                    out.push_str("\\eqref{");
                    out.push_str(&escape_latex(label));
                    out.push('}');
                } else if let Some(prefix) = reference_prefix(label) {
                    out.push_str(prefix);
                    out.push_str("~\\ref{");
                    out.push_str(&escape_latex(label));
                    out.push('}');
                } else {
                    out.push_str("\\ref{");
                    out.push_str(&escape_latex(label));
                    out.push('}');
                }
            }
            Inline::Label(label) => {
                out.push_str("\\label{");
                out.push_str(&escape_latex(label));
                out.push('}');
            }
            Inline::Cite(key) => {
                let cmd = options.cite_command.as_deref().unwrap_or("cite");
                out.push_str("\\");
                out.push_str(cmd);
                out.push_str("{");
                out.push_str(&escape_latex(key));
                out.push('}');
            }
            Inline::Footnote(content) => {
                out.push_str("\\footnote{");
                out.push_str(&render_inlines(content, options));
                out.push('}');
            }
            Inline::Color { color, content } => {
                let (model, value) = color_to_latex(color);
                out.push_str("\\textcolor");
                if let Some(model) = model {
                    out.push_str(&format!("[{}]", model));
                }
                out.push_str("{");
                out.push_str(&value);
                out.push_str("}{");
                out.push_str(&render_inlines(content, options));
                out.push('}');
            }
            Inline::RawLatex(raw) => out.push_str(raw),
            Inline::Superscript(content) => {
                out.push_str("\\textsuperscript{");
                out.push_str(&render_inlines(content, options));
                out.push('}');
            }
            Inline::Subscript(content) => {
                out.push_str("\\textsubscript{");
                out.push_str(&render_inlines(content, options));
                out.push('}');
            }
            Inline::LineBreak => {
                if !last_was_linebreak {
                    if !out.trim().is_empty() {
                        // Add a trailing space to avoid "\\[...]" being parsed as an optional arg.
                        out.push_str("\\\\ ");
                        last_was_linebreak = true;
                    }
                }
                continue;
            }
        }
        last_was_linebreak = false;
    }
    out
}

fn is_equation_label(label: &str) -> bool {
    let lowered = label.trim().to_lowercase();
    lowered.starts_with("eq:")
}

fn reference_prefix(label: &str) -> Option<&'static str> {
    let lowered = label.trim().to_lowercase();
    if lowered.starts_with("fig:") {
        return Some("Fig.");
    }
    if lowered.starts_with("tab:") {
        return Some("Table");
    }
    if lowered.starts_with("sec:") {
        return Some("Section");
    }
    if lowered.starts_with("alg:") {
        return Some("Algorithm");
    }
    if lowered.starts_with("lst:") {
        return Some("Listing");
    }
    if lowered.starts_with("thm:") {
        return Some("Theorem");
    }
    if lowered.starts_with("lemma:") || lowered.starts_with("lem:") {
        return Some("Lemma");
    }
    if lowered.starts_with("prop:") {
        return Some("Proposition");
    }
    if lowered.starts_with("def:") {
        return Some("Definition");
    }
    if lowered.starts_with("cor:") {
        return Some("Corollary");
    }
    if lowered.starts_with("ex:") {
        return Some("Example");
    }
    if lowered.starts_with("remark:") || lowered.starts_with("rem:") {
        return Some("Remark");
    }
    if lowered.starts_with("app:") || lowered.starts_with("appendix:") {
        return Some("Appendix");
    }
    None
}

fn escape_latex(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{") ,
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$") ,
            '&' => out.push_str("\\&") ,
            '%' => out.push_str("\\%") ,
            '#' => out.push_str("\\#") ,
            '_' => out.push_str("\\_") ,
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            _ => out.push(ch),
        }
    }
    out
}

fn color_to_latex(input: &str) -> (Option<&'static str>, String) {
    let trimmed = input.trim().trim_matches('"');
    if let Some(hex) = extract_hex_color(trimmed) {
        return (Some("HTML"), hex);
    }
    (None, escape_latex(trimmed))
}

fn normalize_inline_whitespace(input: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

fn render_table(table: &Table, options: Option<&LatexRenderOptions>) -> String {
    let default_opts;
    let opts = if let Some(opts) = options {
        opts
    } else {
        default_opts = LatexRenderOptions::default();
        &default_opts
    };
    let mut out = String::new();
    let mut has_style = false;
    if table.inset.is_some() || table.stroke.is_some() || table.fill.is_some() {
        has_style = true;
    }
    let stroke_value = table.stroke.as_deref();
    let stroke_enabled = match stroke_value {
        Some(value) => !stroke_is_none(value),
        None => true, // Typst tables default to a visible stroke.
    };
    let stroke = stroke_value.filter(|value| !stroke_is_none(value));
    let stroke_width = stroke.and_then(parse_stroke_width);
    let grid_lines = match opts.table_style {
        TableStyle::Grid => true,
        TableStyle::Booktabs => false,
        TableStyle::Plain => opts.table_grid || stroke_enabled,
    };
    let use_booktabs = opts.table_style == TableStyle::Booktabs;
    if has_style {
        out.push_str("\\begingroup\n");
        if let Some(inset) = table.inset.as_deref() {
            out.push_str(&format!("\\setlength{{\\tabcolsep}}{{{}}}\n", inset));
        }
        if let Some(stroke) = stroke_width.as_deref() {
            out.push_str(&format!("\\setlength{{\\arrayrulewidth}}{{{}}}\n", stroke));
        }
        if let Some(fill) = table.fill.as_deref() {
            if let Some(row_colors) = parse_row_colors(fill) {
                let (odd, odd_def) = resolve_color(&row_colors.odd, "tylaxOddRow");
                let (even, even_def) = resolve_color(&row_colors.even, "tylaxEvenRow");
                if let Some(def) = odd_def {
                    out.push_str(&def);
                    out.push('\n');
                }
                if let Some(def) = even_def {
                    out.push_str(&def);
                    out.push('\n');
                }
                if let Some(header) = row_colors.header.as_deref() {
                    let (header_name, header_def) = resolve_color(header, "tylaxHeaderRow");
                    if let Some(def) = header_def {
                        out.push_str(&def);
                        out.push('\n');
                    }
                    out.push_str(&format!("\\rowcolors{{2}}{{{}}}{{{}}}\n", odd, even));
                    out.push_str(&format!("\\def\\tylaxHeaderRowColor{{{}}}\n", header_name));
                } else {
                    out.push_str(&format!("\\rowcolors{{1}}{{{}}}{{{}}}\n", odd, even));
                }
            } else if fill.contains("=>") {
                // Skip complex function-based fills that aren't recognized row/odd patterns.
            } else {
                let (color_name, define) = resolve_color(fill, "tylaxTableFill");
                if let Some(def) = define {
                    out.push_str(&def);
                    out.push('\n');
                }
                out.push_str(&format!(
                    "\\rowcolors{{1}}{{{}}}{{{}}}\n",
                    color_name, color_name
                ));
            }
        }
    }
    let col_spec = build_column_spec(table, grid_lines);
    out.push_str(&format!("\\begin{{tabular}}{{{}}}\n", col_spec));
    if grid_lines {
        out.push_str("\\hline\n");
    } else if use_booktabs {
        out.push_str("\\toprule\n");
    }

    let columns = table.columns.max(1);
    let mut col_idx = 0usize;
    let mut skip: Vec<usize> = vec![0; columns];
    let mut row_cells: Vec<String> = Vec::new();
    let mut rows: Vec<(String, bool)> = Vec::new();
    let mut row_has_header = false;

    let flush_row = |row_cells: &mut Vec<String>, rows: &mut Vec<(String, bool)>, row_has_header: &mut bool| {
        if !row_cells.is_empty() {
            rows.push((row_cells.join(" & "), *row_has_header));
            row_cells.clear();
            *row_has_header = false;
        }
    };

    for cell in &table.cells {
        // Move to next row if we have filled columns.
        if col_idx >= columns {
            flush_row(&mut row_cells, &mut rows, &mut row_has_header);
            col_idx = 0;
        }

        // Insert placeholders for covered columns from rowspans.
        while col_idx < columns && skip[col_idx] > 0 {
            skip[col_idx] -= 1;
            row_cells.push(String::new());
            col_idx += 1;
            if col_idx >= columns {
                flush_row(&mut row_cells, &mut rows, &mut row_has_header);
                col_idx = 0;
            }
        }

        let rendered = normalize_inline_whitespace(&render_inlines(&cell.content, opts));
        let rendered = apply_cell_style(cell, &rendered);
        let rendered = apply_cell_header(cell, &rendered);
        let rendered = apply_cell_alignment(cell, &rendered, col_idx, table);
        let rendered = apply_cell_spans(cell, &rendered, col_idx, table, grid_lines);
        if cell.is_header {
            row_has_header = true;
        }
        row_cells.push(rendered);

        if cell.rowspan > 1 {
            for i in 0..cell.colspan.max(1) {
                if col_idx + i < columns {
                    skip[col_idx + i] = skip[col_idx + i].max(cell.rowspan - 1);
                }
            }
        }

        col_idx += cell.colspan.max(1);
        if col_idx >= columns {
            flush_row(&mut row_cells, &mut rows, &mut row_has_header);
            col_idx = 0;
        }
    }

    flush_row(&mut row_cells, &mut rows, &mut row_has_header);

    if !rows.is_empty() {
        if has_style && out.contains("\\tylaxHeaderRowColor") {
            if let Some(first) = rows.first_mut() {
                let header = format!("\\rowcolor{{\\tylaxHeaderRowColor}} {}", first.0);
                first.0 = header;
            }
        }
        let mut midrule_added = false;
        for (row, is_header) in rows {
            out.push_str(&row);
            out.push_str(" \\\\\n");
            if grid_lines {
                out.push_str("\\hline\n");
            } else if use_booktabs && is_header && !midrule_added {
                out.push_str("\\midrule\n");
                midrule_added = true;
            }
        }
        if use_booktabs {
            out.push_str("\\bottomrule\n");
        }
    }

    if has_style {
        out.push_str("\\end{tabular}\n");
        out.push_str("\\endgroup");
    } else {
        out.push_str("\\end{tabular}");
    }

    if let Some(caption) = &table.caption {
        out.push_str("\n");
        out.push_str("\\caption{");
        out.push_str(&normalize_inline_whitespace(&render_inlines(caption, opts)));
        out.push_str("}");
    }

    out
}

fn render_table_block(
    table: &Table,
    options: &LatexRenderOptions,
    label: Option<&str>,
) -> String {
    let has_caption = table.caption.is_some();
    let has_label = label.is_some();
    if !has_caption && !has_label {
        return render_table(table, Some(options));
    }
    let caption_first = options.table_caption_position == TableCaptionPosition::Top;
    let label_only = has_label && !has_caption;
    let mut out = String::new();
    let placement = if options.force_here && !options.two_column {
        "H"
    } else {
        "h"
    };
    out.push_str(&format!("\\begin{{table}}[{}]\n\\centering\n", placement));

    let mut table_body = table.clone();
    if has_caption {
        table_body.caption = None;
    }

    let render_label = |out: &mut String| {
        if let Some(label) = label {
            if has_caption {
                out.push_str("\\label{");
                out.push_str(&escape_latex(label));
                out.push_str("}\n");
            } else {
                out.push_str("\\refstepcounter{table}\n\\label{");
                out.push_str(&escape_latex(label));
                out.push_str("}\n");
            }
        }
    };

    if caption_first {
        if let Some(caption) = &table.caption {
            out.push_str("\\caption{");
            out.push_str(&normalize_inline_whitespace(&render_inlines(caption, options)));
            out.push_str("}\n");
        }
        render_label(&mut out);
    } else if label_only {
        render_label(&mut out);
    }

    out.push_str(&render_table(&table_body, Some(options)));

    if !caption_first {
        if let Some(caption) = &table.caption {
            out.push_str("\n\\caption{");
            out.push_str(&normalize_inline_whitespace(&render_inlines(caption, options)));
            out.push_str("}");
        }
        if has_label && !label_only {
            out.push('\n');
            render_label(&mut out);
        }
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\\end{table}");
    out
}

fn stroke_is_none(value: &str) -> bool {
    let trimmed = value.trim().trim_matches('"').to_lowercase();
    trimmed == "none" || trimmed == "0" || trimmed == "0pt"
}

fn parse_stroke_width(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() || stroke_is_none(trimmed) {
        return None;
    }
    if trimmed.contains("=>") {
        return None;
    }
    let mut chars = trimmed.chars().peekable();
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut num_len = 0usize;
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            chars.next();
            num_len += ch.len_utf8();
            continue;
        }
        if ch == '.' && !seen_dot {
            seen_dot = true;
            chars.next();
            num_len += ch.len_utf8();
            continue;
        }
        break;
    }
    if !seen_digit {
        return None;
    }
    let rest = trimmed[num_len..].trim_start();
    if rest.is_empty() {
        return None;
    }
    let unit: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if unit.is_empty() {
        return None;
    }
    Some(format!("{}{}", &trimmed[..num_len], unit))
}

fn build_column_spec(table: &Table, grid_lines: bool) -> String {
    let mut spec = String::new();
    let columns = table.columns.max(1);
    let align = table.align.clone().unwrap_or_default();
    if grid_lines {
        spec.push('|');
    }
    for i in 0..columns {
        let a = align.get(i).copied().unwrap_or(Alignment::Center);
        spec.push(match a {
            Alignment::Left => 'l',
            Alignment::Right => 'r',
            Alignment::Center => 'c',
        });
        if grid_lines {
            spec.push('|');
        }
    }
    spec
}

fn apply_cell_spans(
    cell: &TableCell,
    content: &str,
    col_idx: usize,
    table: &Table,
    grid_lines: bool,
) -> String {
    let mut rendered = content.to_string();
    if cell.colspan > 1 {
        let mut spec = column_align_spec(cell, col_idx, table).to_string();
        if grid_lines {
            spec = format!("|{}|", spec);
        }
        rendered = format!(
            "\\multicolumn{{{}}}{{{}}}{{{}}}",
            cell.colspan, spec, rendered
        );
    }
    if cell.rowspan > 1 {
        rendered = format!("\\multirow{{{}}}{{*}}{{{}}}", cell.rowspan, rendered);
    }
    rendered
}

fn apply_cell_alignment(cell: &TableCell, content: &str, col_idx: usize, table: &Table) -> String {
    if cell.colspan > 1 {
        return content.to_string();
    }
    let align = cell
        .align
        .or_else(|| table.align.as_ref().and_then(|a| a.get(col_idx).copied()))
        .unwrap_or(Alignment::Center);
    match align {
        Alignment::Left => format!("\\raggedright {}", content),
        Alignment::Right => format!("\\raggedleft {}", content),
        Alignment::Center => content.to_string(),
    }
}

fn apply_cell_style(cell: &TableCell, content: &str) -> String {
    if let Some(fill) = cell.fill.as_deref() {
        let (color_name, define) = resolve_color(fill, "tylaxCellFill");
        if let Some(def) = define {
            return format!("{}\\cellcolor{{{}}} {}", def, color_name, content);
        }
        return format!("\\cellcolor{{{}}} {}", color_name, content);
    }
    content.to_string()
}

fn apply_cell_header(cell: &TableCell, content: &str) -> String {
    if cell.is_header {
        return format!("\\textbf{{{}}}", content);
    }
    content.to_string()
}

fn column_align_spec(cell: &TableCell, col_idx: usize, table: &Table) -> char {
    let align = cell
        .align
        .or_else(|| table.align.as_ref().and_then(|a| a.get(col_idx).copied()))
        .unwrap_or(Alignment::Center);
    match align {
        Alignment::Left => 'l',
        Alignment::Right => 'r',
        Alignment::Center => 'c',
    }
}

fn render_figure(figure: &Figure, options: &LatexRenderOptions) -> String {
    if options.inline_wide_tables && options.two_column {
        if let FigureContent::Table(table) = &figure.content {
            if is_wide_table(table) {
                let mut out = String::new();
                out.push_str("\\begin{center}\n");
                let caption_first =
                    options.table_caption_position == TableCaptionPosition::Top;
                if caption_first {
                    if let Some(caption) = &figure.caption {
                        out.push_str("\\captionof{table}{");
                        out.push_str(&normalize_inline_whitespace(&render_inlines(caption, options)));
                        out.push_str("}");
                    }
                    if let Some(label) = &figure.label {
                        out.push_str("\n\\label{");
                        out.push_str(&escape_latex(label));
                        out.push_str("}");
                    }
                    out.push('\n');
                }
                out.push_str(&render_table(table, Some(options)));
                if !caption_first {
                    if let Some(caption) = &figure.caption {
                        out.push_str("\n\\captionof{table}{");
                        out.push_str(&normalize_inline_whitespace(&render_inlines(caption, options)));
                        out.push_str("}");
                    }
                    if let Some(label) = &figure.label {
                        out.push_str("\n\\label{");
                        out.push_str(&escape_latex(label));
                        out.push_str("}");
                    }
                }
                out.push_str("\n\\end{center}");
                return out;
            }
        }
    }

    let mut out = String::new();
    let mapped = figure.placement.as_deref().and_then(map_placement);
    let mut placement = mapped.unwrap_or("h");
    if options.force_here && !options.two_column {
        let should_force = figure.placement.is_none() || placement == "h";
        if should_force {
            placement = "H";
        }
    }
    let base_env = match figure.content {
        FigureContent::Table(_) => "table",
        _ => "figure",
    };
    let mut env = base_env.to_string();
    let mut placement = placement.to_string();
    if options.two_column {
        let wide = match &figure.content {
            FigureContent::Image(image) => is_wide_image(image),
            FigureContent::Table(table) => is_wide_table(table),
            FigureContent::Raw(_) => false,
        };
        if wide {
            env.push('*');
            if placement == "h" {
                placement = "t".to_string();
            }
        }
    }
    out.push_str(&format!("\\begin{{{}}}[{}]\n\\centering\n", env, placement));

    let caption_first = matches!(
        figure.content,
        FigureContent::Table(_)
    ) && options.table_caption_position == TableCaptionPosition::Top;
    if caption_first {
        if let Some(caption) = &figure.caption {
            out.push_str("\\caption{");
            out.push_str(&normalize_inline_whitespace(&render_inlines(caption, options)));
            out.push_str("}");
        }
        if let Some(label) = &figure.label {
            out.push_str("\n\\label{");
            out.push_str(&escape_latex(label));
            out.push_str("}");
        }
        out.push('\n');
    }

    match &figure.content {
        FigureContent::Table(table) => {
            out.push_str(&render_table(table, Some(options)));
        }
        FigureContent::Image(image) => {
            out.push_str(&render_image(image));
        }
        FigureContent::Raw(blocks) => {
            out.push_str(&render_blocks_inline(blocks, options));
        }
    }

    if !caption_first {
        if let Some(caption) = &figure.caption {
            out.push_str("\n\\caption{");
            out.push_str(&normalize_inline_whitespace(&render_inlines(caption, options)));
            out.push_str("}");
        }
        if let Some(label) = &figure.label {
            out.push_str("\n\\label{");
            out.push_str(&escape_latex(label));
            out.push_str("}");
        }
    }

    out.push_str(&format!("\n\\end{{{}}}", env));
    out
}

fn is_wide_image(image: &Image) -> bool {
    let Some(width) = image.width.as_deref() else {
        return false;
    };
    if let Some(ratio) = parse_width_ratio(width) {
        return ratio >= 0.95;
    }
    let lowered = width.to_lowercase();
    lowered.contains("linewidth") || lowered.contains("textwidth")
}

fn parse_width_ratio(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    let percent = trimmed.strip_suffix('%')?;
    let val = percent.trim().parse::<f64>().ok()?;
    Some((val / 100.0).clamp(0.0, 10.0))
}

fn is_wide_table(table: &Table) -> bool {
    // Be conservative: only force table* when the column count is clearly wide.
    table.columns >= 6
}

fn render_image(image: &Image) -> String {
    let mut opts = Vec::new();
    if let Some(width) = image.width.as_deref().and_then(convert_length_to_latex) {
        opts.push(format!("width={}", width));
    }
    if let Some(height) = image.height.as_deref().and_then(convert_length_to_latex) {
        opts.push(format!("height={}", height));
    }
    if let Some(fit) = image.fit.as_deref() {
        if fit.contains("contain") || fit.contains("fit") {
            opts.push("keepaspectratio".to_string());
        }
    }
    let opt_str = if opts.is_empty() {
        String::new()
    } else {
        format!("[{}]", opts.join(","))
    };
    format!(
        "\\includegraphics{}{{{}}}",
        opt_str,
        escape_latex(&image.path)
    )
}

fn map_placement(raw: &str) -> Option<&'static str> {
    match raw.trim() {
        "top" | "t" => Some("t"),
        "bottom" | "b" => Some("b"),
        "here" | "h" => Some("h"),
        "page" | "p" => Some("p"),
        "none" => Some("h"),
        _ => None,
    }
}

fn convert_length_to_latex(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        let val = percent.trim().parse::<f64>().ok()?;
        if (val - 100.0).abs() < 0.1 {
            return Some("\\linewidth".to_string());
        }
        let scale = val / 100.0;
        return Some(format!("{:.2}\\linewidth", scale));
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.')
    {
        return Some(format!("{}pt", trimmed));
    }
    Some(trimmed.to_string())
}

fn render_bibliography(file: &str, style: Option<&str>) -> String {
    let mut files = Vec::new();
    for part in file.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let trimmed = trimmed.trim_end_matches(".bib");
        if !trimmed.is_empty() {
            files.push(escape_latex(trimmed));
        }
    }
    let joined = if files.is_empty() {
        escape_latex(file.trim())
    } else {
        files.join(",")
    };
    let style = style
        .map(map_bibliography_style)
        .unwrap_or_else(|| "plain".to_string());
    let mut out = String::new();
    out.push_str(&format!("\\bibliographystyle{{{}}}\n", style));
    out.push_str("\\bibliography{");
    out.push_str(&joined);
    out.push('}');
    out
}

fn render_vspace(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "\\par\\vspace{0pt}".to_string();
    }
    if let Some(converted) = convert_vspace_length(trimmed) {
        return format!("\\par\\vspace{{{}}}", converted);
    }
    format!("\\par\\vspace{{{}}}", trimmed)
}

fn convert_vspace_length(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(fr) = trimmed.strip_suffix("fr") {
        let val = fr.trim().parse::<f64>().ok()?;
        if (val - 1.0).abs() < 0.01 {
            return Some("\\fill".to_string());
        }
        if val > 0.0 {
            if (val.fract()).abs() < 0.01 {
                return Some(format!("\\stretch{{{}}}", val.round() as i64));
            }
            return Some(format!("\\stretch{{{:.2}}}", val));
        }
    }
    if trimmed
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == '.')
    {
        return Some(format!("{}pt", trimmed));
    }
    Some(trimmed.to_string())
}

fn map_bibliography_style(raw: &str) -> String {
    let lowered = raw.trim().to_lowercase();
    if lowered.contains("ieee") {
        return "IEEEtran".to_string();
    }
    if lowered.contains("acm") {
        return "ACM-Reference-Format".to_string();
    }
    if lowered.contains("apa") {
        return "apalike".to_string();
    }
    if lowered.contains("springer") && lowered.contains("mathphys") {
        return "spmpsci".to_string();
    }
    raw.trim().to_string()
}

fn render_outline(title: Option<&[Inline]>, options: &LatexRenderOptions) -> String {
    let mut out = String::new();
    if let Some(title) = title {
        out.push_str("\\renewcommand{\\contentsname}{");
        out.push_str(&normalize_inline_whitespace(&render_inlines(title, options)));
        out.push_str("}\n");
    }
    out.push_str("\\tableofcontents");
    out
}

fn render_box(blocks: &[Block], options: &LatexRenderOptions) -> String {
    let mut out = String::new();
    out.push_str("\\fbox{");
    out.push_str(&render_blocks_inline(blocks, options));
    out.push('}');
    out
}

fn render_block_wrapper(blocks: &[Block], options: &LatexRenderOptions) -> String {
    render_blocks_inline(blocks, options)
}

fn render_columns(columns: &tylax_ir::Columns, options: &LatexRenderOptions) -> String {
    let mut out = String::new();
    out.push_str(&format!("\\begin{{multicols}}{{{}}}\n", columns.columns.max(1)));
    out.push_str(&render_blocks_inline(&columns.blocks, options));
    out.push_str("\n\\end{multicols}");
    out
}

fn render_grid(grid: &Grid, options: &LatexRenderOptions) -> String {
    let mut out = String::new();
    let col_gutter = grid
        .column_gutter
        .as_deref()
        .or(grid.gutter.as_deref());
    let row_gutter = grid.row_gutter.as_deref().or(grid.gutter.as_deref());
    let has_style = col_gutter.is_some() || row_gutter.is_some();
    if has_style {
        out.push_str("\\begingroup\n");
    }
    if let Some(gutter) = col_gutter {
        out.push_str(&format!("\\setlength{{\\tabcolsep}}{{{}}}\n", gutter));
    }
    if let Some(gutter) = row_gutter {
        out.push_str(&format!("\\setlength{{\\extrarowheight}}{{{}}}\n", gutter));
    }
    let columns = grid.columns.max(1);
    out.push_str(&format!("\\begin{{tabular}}{{{}}}\n", "c".repeat(columns)));

    let mut col_idx = 0usize;
    let mut row_cells: Vec<String> = Vec::new();
    let mut rows: Vec<String> = Vec::new();

    let flush_row = |row_cells: &mut Vec<String>, rows: &mut Vec<String>| {
        if !row_cells.is_empty() {
            rows.push(row_cells.join(" & "));
            row_cells.clear();
        }
    };

    for cell in &grid.cells {
        if col_idx >= columns {
            flush_row(&mut row_cells, &mut rows);
            col_idx = 0;
        }
        row_cells.push(render_blocks_inline(cell, options));
        col_idx += 1;
        if col_idx >= columns {
            flush_row(&mut row_cells, &mut rows);
            col_idx = 0;
        }
    }

    flush_row(&mut row_cells, &mut rows);
    if !rows.is_empty() {
        out.push_str(&rows.join(" \\\\\n"));
        out.push_str(" \\\\\n");
    }
    if has_style {
        out.push_str("\\end{tabular}\n");
        out.push_str("\\endgroup");
    } else {
        out.push_str("\\end{tabular}");
    }
    out
}

fn resolve_color(raw: &str, default_name: &str) -> (String, Option<String>) {
    if let Some(hex) = extract_hex_color(raw) {
        let name = default_name.to_string();
        let def = format!("\\definecolor{{{}}}{{HTML}}{{{}}}", name, hex);
        return (name, Some(def));
    }
    let name = sanitize_color_name(raw);
    (name, None)
}

struct RowColors {
    header: Option<String>,
    odd: String,
    even: String,
}

fn parse_row_colors(raw: &str) -> Option<RowColors> {
    let tokens = extract_color_tokens(raw);
    if tokens.is_empty() {
        return None;
    }

    let lower = raw.to_lowercase();
    let header_color = lower.contains("row == 0")
        || lower.contains("row==0")
        || lower.contains("y == 0")
        || lower.contains("y==0");
    let header_skip = lower.contains("row > 0")
        || lower.contains("row>0")
        || lower.contains("y > 0")
        || lower.contains("y>0");
    if header_color {
        let header = tokens
            .get(0)
            .cloned()
            .unwrap_or_else(|| "white".to_string());
        let odd = tokens
            .get(1)
            .cloned()
            .unwrap_or_else(|| "white".to_string());
        let even = tokens
            .get(2)
            .cloned()
            .unwrap_or_else(|| odd.clone());
        return Some(RowColors {
            header: Some(header),
            odd,
            even,
        });
    }

    let odd_even_pattern = lower.contains("calc.odd(")
        || lower.contains("odd(")
        || lower.contains("row % 2")
        || lower.contains("y % 2")
        || lower.contains("calc.rem(");
    if odd_even_pattern {
        let mut odd = tokens
            .get(0)
            .cloned()
            .unwrap_or_else(|| "white".to_string());
        let mut even = tokens.get(1).cloned().unwrap_or_else(|| odd.clone());
        let even_pattern = lower.contains("== 0") || lower.contains("!= 1") || lower.contains("even");
        let odd_pattern = lower.contains("== 1") || lower.contains("!= 0");
        if tokens.len() == 1 {
            if even_pattern && !odd_pattern {
                even = odd.clone();
                odd = "white".to_string();
            } else {
                even = "white".to_string();
            }
        } else if even_pattern && !odd_pattern {
            std::mem::swap(&mut odd, &mut even);
        }
        return Some(RowColors {
            header: if header_skip {
                Some("white".to_string())
            } else {
                None
            },
            odd,
            even,
        });
    }

    None
}

fn extract_color_tokens(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut remaining = raw;
    while let Some(start) = remaining.find('{') {
        let after = &remaining[start + 1..];
        if let Some(end) = after.find('}') {
            let inner = after[..end].trim();
            if !inner.is_empty() {
                tokens.push(inner.to_string());
            }
            remaining = &after[end + 1..];
        } else {
            break;
        }
    }
    tokens
}

fn extract_hex_color(raw: &str) -> Option<String> {
    let s = raw.trim();
    if let Some(idx) = s.find('#') {
        let mut hex = String::new();
        for ch in s[idx + 1..].chars() {
            if ch.is_ascii_hexdigit() {
                hex.push(ch.to_ascii_uppercase());
                if hex.len() == 6 {
                    break;
                }
            } else {
                break;
            }
        }
        if hex.len() == 3 || hex.len() == 6 {
            return Some(hex);
        }
    }
    None
}

fn sanitize_color_name(raw: &str) -> String {
    let mut name = String::new();
    let trimmed = raw
        .trim()
        .trim_start_matches("rgb(")
        .trim_start_matches("rgba(")
        .trim_end_matches(')')
        .trim_matches('"');
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch);
        }
    }
    if name.is_empty() {
        "black".to_string()
    } else {
        name
    }
}
