use std::collections::BTreeMap;

use typst_syntax::{parse, SyntaxKind, SyntaxNode};

#[derive(Debug, Default, Clone)]
pub struct PreambleHints {
    pub paper: Option<String>,
    pub margin: Margin,
    pub text_size: Option<String>,
    pub font: Option<String>,
    pub justify: Option<bool>,
    pub leading: Option<String>,
    pub first_line_indent: Option<String>,
    pub columns: Option<usize>,
    pub bibliography_style: Option<String>,
    pub colors: BTreeMap<String, String>,
    pub heading_styles: BTreeMap<u8, HeadingStyle>,
    pub equation_numbering: Option<String>,
    pub uses_natbib: bool,
    pub uses_amsthm: bool,
    pub has_headings: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Margin {
    pub all: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct HeadingStyle {
    pub size: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub before: Option<String>,
    pub after: Option<String>,
}

pub fn extract_preamble_hints(input: &str) -> PreambleHints {
    let root = parse(input);
    let mut hints = PreambleHints::default();

    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            SyntaxKind::SetRule => {
                if let Some(name) = set_rule_name(&node) {
                    match name.as_str() {
                        "page" => parse_page_set(&node, &mut hints),
                        "text" => parse_text_set(&node, &mut hints),
                        "par" => parse_par_set(&node, &mut hints),
                        "math.equation" => parse_math_equation_set(&node, &mut hints),
                        "std.bibliography" => parse_bibliography_set(&node, &mut hints),
                        _ => {}
                    }
                }
            }
            SyntaxKind::Heading => {
                hints.has_headings = true;
            }
            SyntaxKind::ShowRule => {
                parse_heading_show_rule(&node, &mut hints);
            }
            SyntaxKind::FuncCall => {
                if let Some(name) = get_func_call_name(&node) {
                    if name == "cite" && cite_uses_natbib(&node) {
                        hints.uses_natbib = true;
                    }
                    if is_theorem_like(&name) {
                        hints.uses_amsthm = true;
                    }
                }
            }
            SyntaxKind::LetBinding => {
                if let Some((name, hex)) = parse_color_let(&node) {
                    hints.colors.insert(name, hex);
                }
            }
            _ => {}
        }
        for child in node.children() {
            stack.push(child.clone());
        }
    }

    hints
}

pub fn render_article_preamble(hints: &PreambleHints) -> String {
    let mut out = String::new();

    let mut class_opts = Vec::new();
    if let Some(size) = hints.text_size.as_deref() {
        if let Some(opt) = map_text_size_option(size) {
            class_opts.push(opt.to_string());
        }
    }
    if let Some(paper) = hints.paper.as_deref() {
        if let Some(opt) = map_paper_option(paper) {
            class_opts.push(opt.to_string());
        }
    }

    if class_opts.is_empty() {
        out.push_str("\\documentclass{article}\n");
    } else {
        out.push_str(&format!(
            "\\documentclass[{}]{{article}}\n",
            class_opts.join(",")
        ));
    }

    if let Some(geometry) = build_geometry_options(&hints.margin) {
        out.push_str(&format!("\\usepackage[{}]{{geometry}}\n", geometry));
    }

    out.push_str("\\usepackage{amsmath,amssymb}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{hyperref}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    out.push_str("\\usepackage{enumitem}\n");
    out.push_str("\\usepackage{multirow}\n");
    out.push_str("\\usepackage{multicol}\n");
    out.push_str("\\usepackage{array}\n");
    out.push_str("\\usepackage{textcomp}\n");
    if hints.uses_natbib {
        out.push_str("\\usepackage{natbib}\n");
    }
    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
    }

    if !hints.heading_styles.is_empty() {
        out.push_str("\\usepackage{titlesec}\n");
        for (level, style) in &hints.heading_styles {
            if let Some(cmd) = heading_command(*level) {
                let format = render_heading_format(style);
                out.push_str(&format!(
                    "\\titleformat{{\\{}}}{{{}}}{{\\the{}}}{{1em}}{{}}\n",
                    cmd,
                    format,
                    cmd
                ));
                if style.before.is_some() || style.after.is_some() {
                    let before = style.before.as_deref().unwrap_or("0pt");
                    let after = style.after.as_deref().unwrap_or("0pt");
                    out.push_str(&format!(
                        "\\titlespacing*{{\\{}}}{{0pt}}{{{}}}{{{}}}\n",
                        cmd, before, after
                    ));
                }
            }
        }
    }

    if let Some(font) = hints.font.as_deref() {
        if is_new_computer_modern(font) {
            out.push_str("\\usepackage{newcomputermodern}\n");
        } else {
            out.push_str("\\usepackage{iftex}\n");
            out.push_str("\\ifPDFTeX\n");
            out.push_str("\\else\n");
            out.push_str("\\usepackage{fontspec}\n");
            out.push_str(&format!("\\setmainfont{{{}}}\n", escape_latex(font)));
            out.push_str("\\fi\n");
        }
    }

    if let Some(within) = equation_number_within(hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }

    if hints.uses_amsthm {
        out.push_str(&render_amsthm_definitions(hints));
    }

    if let Some(stretch) = compute_line_stretch(hints) {
        out.push_str("\\usepackage{setspace}\n");
        out.push_str(&format!("\\setstretch{{{:.2}}}\n", stretch));
    }

    for (name, hex) in &hints.colors {
        out.push_str(&format!(
            "\\definecolor{{{}}}{{HTML}}{{{}}}\n",
            escape_latex(name),
            escape_latex(hex)
        ));
    }

    if let Some(indent) = hints.first_line_indent.as_deref() {
        out.push_str(&format!("\\setlength{{\\parindent}}{{{}}}\n", indent));
    }

    if hints.justify == Some(false) {
        out.push_str("\\AtBeginDocument{\\raggedright}\n");
    }

    out.push_str("\\providecommand{\\textsubscript}[1]{$_{\\text{#1}}$}\n");
    out
}

pub fn render_amsthm_definitions(hints: &PreambleHints) -> String {
    if !hints.uses_amsthm {
        return String::new();
    }
    let mut out = String::new();
    let within = theorem_number_within(hints);
    let theorem_decl = match within.as_deref() {
        Some(within) => format!("\\newtheorem{{theorem}}{{Theorem}}[{}]\n", within),
        None => "\\newtheorem{theorem}{Theorem}\n".to_string(),
    };
    out.push_str("\\theoremstyle{plain}\n");
    out.push_str(&theorem_decl);
    out.push_str("\\newtheorem{lemma}[theorem]{Lemma}\n");
    out.push_str("\\newtheorem{corollary}[theorem]{Corollary}\n");
    out.push_str("\\newtheorem{proposition}[theorem]{Proposition}\n");
    out.push_str("\\newtheorem{claim}[theorem]{Claim}\n");
    out.push_str("\\newtheorem{axiom}[theorem]{Axiom}\n");
    out.push_str("\\theoremstyle{definition}\n");
    out.push_str("\\newtheorem{definition}[theorem]{Definition}\n");
    out.push_str("\\newtheorem{example}[theorem]{Example}\n");
    out.push_str("\\theoremstyle{remark}\n");
    out.push_str("\\newtheorem{remark}[theorem]{Remark}\n");
    out
}

fn parse_page_set(node: &SyntaxNode, hints: &mut PreambleHints) {
    let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return;
    };
    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        match key.as_str() {
            "paper" => {
                if let Some(text) = extract_literal_string(&value) {
                    hints.paper = Some(text);
                }
            }
            "margin" => {
                if let Some(margin) = parse_margin_value(&value) {
                    hints.margin = margin;
                }
            }
            "columns" => {
                if let Some(columns) = parse_usize_literal(&value) {
                    if columns >= 1 {
                        hints.columns = Some(columns);
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_text_set(node: &SyntaxNode, hints: &mut PreambleHints) {
    let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return;
    };
    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        match key.as_str() {
            "size" => {
                if let Some(text) = extract_literal_string(&value) {
                    hints.text_size = Some(text);
                }
            }
            "font" => {
                if let Some(text) = extract_literal_string(&value) {
                    hints.font = Some(text);
                }
            }
            _ => {}
        }
    }
}

fn parse_par_set(node: &SyntaxNode, hints: &mut PreambleHints) {
    let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return;
    };
    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        match key.as_str() {
            "justify" => {
                hints.justify = parse_bool_literal(&value);
            }
            "leading" => {
                if let Some(text) = extract_literal_string(&value) {
                    hints.leading = Some(text);
                }
            }
            "first-line-indent" => {
                if let Some(text) = extract_literal_string(&value) {
                    hints.first_line_indent = Some(text);
                }
            }
            _ => {}
        }
    }
}

fn parse_bibliography_set(node: &SyntaxNode, hints: &mut PreambleHints) {
    let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return;
    };
    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        if key == "style" {
            if let Some(text) = extract_literal_string(&value) {
                hints.bibliography_style = Some(text.clone());
                if bibliography_style_needs_natbib(&text) {
                    hints.uses_natbib = true;
                }
            }
        }
    }
}

fn parse_math_equation_set(node: &SyntaxNode, hints: &mut PreambleHints) {
    let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return;
    };
    for child in args.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        if key == "numbering" {
            if let Some(text) = extract_literal_string(&value) {
                hints.equation_numbering = Some(text);
            }
        }
    }
}

fn parse_margin_value(node: &SyntaxNode) -> Option<Margin> {
    match node.kind() {
        SyntaxKind::Numeric | SyntaxKind::Str => {
            let value = extract_literal_string(node)?;
            return Some(Margin {
                all: Some(value),
                ..Margin::default()
            });
        }
        SyntaxKind::Dict => {
            let mut margin = Margin::default();
            for child in node.children() {
                if child.kind() != SyntaxKind::Named {
                    continue;
                }
                let key = extract_named_key(&child).unwrap_or_default();
                let Some(value) = extract_named_value_node(&child) else {
                    continue;
                };
                let Some(text) = extract_literal_string(&value) else {
                    continue;
                };
                match key.as_str() {
                    "x" => {
                        margin.left = Some(text.clone());
                        margin.right = Some(text);
                    }
                    "y" => {
                        margin.top = Some(text.clone());
                        margin.bottom = Some(text);
                    }
                    "left" => margin.left = Some(text),
                    "right" => margin.right = Some(text),
                    "top" => margin.top = Some(text),
                    "bottom" => margin.bottom = Some(text),
                    _ => {}
                }
            }
            return Some(margin);
        }
        _ => {}
    }
    None
}

fn build_geometry_options(margin: &Margin) -> Option<String> {
    if let Some(all) = margin.all.as_deref() {
        return Some(format!("margin={}", all));
    }

    let mut opts = Vec::new();
    if let Some(left) = margin.left.as_deref() {
        opts.push(format!("left={}", left));
    }
    if let Some(right) = margin.right.as_deref() {
        opts.push(format!("right={}", right));
    }
    if let Some(top) = margin.top.as_deref() {
        opts.push(format!("top={}", top));
    }
    if let Some(bottom) = margin.bottom.as_deref() {
        opts.push(format!("bottom={}", bottom));
    }
    if opts.is_empty() {
        None
    } else {
        Some(opts.join(","))
    }
}

fn compute_line_stretch(hints: &PreambleHints) -> Option<f64> {
    let leading = hints.leading.as_deref()?;
    let size = hints.text_size.as_deref()?;
    let leading_pt = parse_length_to_pt(leading, size)?;
    let size_pt = parse_length_to_pt(size, size)?;
    if size_pt <= 0.0 {
        return None;
    }
    let stretch = (size_pt + leading_pt) / size_pt;
    let stretch = (stretch * 1.1).min(2.5);
    if (0.9..=2.5).contains(&stretch) {
        Some(stretch)
    } else {
        None
    }
}

pub fn equation_number_within(hints: &PreambleHints) -> Option<&'static str> {
    let pattern = hints.equation_numbering.as_deref()?;
    if pattern.contains('.') {
        let dots = pattern.matches('.').count();
        if dots >= 2 {
            return Some("subsection");
        }
        return Some("section");
    }
    None
}

fn theorem_number_within(hints: &PreambleHints) -> Option<&'static str> {
    equation_number_within(hints).or_else(|| if hints.has_headings { Some("section") } else { None })
}

fn heading_command(level: u8) -> Option<&'static str> {
    match level {
        1 => Some("section"),
        2 => Some("subsection"),
        3 => Some("subsubsection"),
        4 => Some("paragraph"),
        _ => None,
    }
}

fn render_heading_format(style: &HeadingStyle) -> String {
    let mut out = String::new();
    if style.bold {
        out.push_str("\\bfseries");
    }
    if style.italic {
        out.push_str("\\itshape");
    }
    if let Some(size) = style.size.as_deref() {
        if let Some(pt) = parse_length_to_pt(size, "10pt") {
            let baseline = (pt * 1.2).max(pt + 1.0);
            out.push_str(&format!("\\fontsize{{{:.1}pt}}{{{:.1}pt}}\\selectfont", pt, baseline));
        }
    }
    if out.is_empty() {
        out.push_str("\\normalfont");
    }
    out
}

fn parse_heading_show_rule(node: &SyntaxNode, hints: &mut PreambleHints) {
    let Some(target) = show_rule_target(node) else {
        return;
    };
    let levels = match target {
        ShowTarget::Heading(levels) => levels,
        _ => return,
    };

    let mut style = HeadingStyle::default();
    let mut v_spacings: Vec<String> = Vec::new();

    walk_nodes(node, &mut |descendant| {
        if descendant.kind() != SyntaxKind::FuncCall {
            return;
        }
        let Some(name) = get_func_call_name(descendant) else {
            return;
        };
        if name == "v" {
            if let Some(arg) = first_arg_literal(descendant) {
                v_spacings.push(arg);
            }
        } else if name == "text" {
            if text_call_contains_it(descendant) {
                if let Some(text_style) = parse_text_style(descendant) {
                    merge_heading_style(&mut style, text_style);
                }
            }
        }
    });

    if !v_spacings.is_empty() {
        style.before = v_spacings.first().cloned();
        style.after = v_spacings.last().cloned();
    }

    for level in levels {
        let entry = hints
            .heading_styles
            .entry(level)
            .or_insert_with(HeadingStyle::default);
        merge_heading_style(entry, style.clone());
    }
}

enum ShowTarget {
    Heading(Vec<u8>),
    Other,
}

fn show_rule_target(node: &SyntaxNode) -> Option<ShowTarget> {
    let mut iter = node.children();
    while let Some(child) = iter.next() {
        match child.kind() {
            SyntaxKind::Ident => {
                if child.text() == "heading" {
                    return Some(ShowTarget::Heading(vec![1, 2, 3, 4]));
                }
            }
            SyntaxKind::FuncCall => {
                if let Some(name) = get_func_call_name(&child) {
                    if name == "heading.where" {
                        if let Some(level) = extract_level_from_args(&child) {
                            return Some(ShowTarget::Heading(vec![level]));
                        }
                        return Some(ShowTarget::Heading(vec![1, 2, 3, 4]));
                    }
                    if name == "heading" {
                        return Some(ShowTarget::Heading(vec![1, 2, 3, 4]));
                    }
                }
            }
            _ => {}
        }
    }
    Some(ShowTarget::Other)
}

fn extract_level_from_args(node: &SyntaxNode) -> Option<u8> {
    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;
    for child in args.children() {
        if child.kind() == SyntaxKind::Named {
            let key = extract_named_key(&child).unwrap_or_default();
            if key == "level" {
                let value = extract_named_value_node(&child)?;
                if let Some(num) = parse_number(&value.text()) {
                    return Some(num.round().max(1.0) as u8);
                }
            }
        }
    }
    None
}

fn first_arg_literal(node: &SyntaxNode) -> Option<String> {
    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;
    for child in args.children() {
        match child.kind() {
            SyntaxKind::Numeric | SyntaxKind::Int | SyntaxKind::Float | SyntaxKind::Str => {
                return Some(child.text().trim_matches('"').to_string());
            }
            _ => {}
        }
    }
    None
}

fn text_call_contains_it(node: &SyntaxNode) -> bool {
    let mut found = false;
    walk_nodes(node, &mut |child| {
        if child.kind() == SyntaxKind::Ident && child.text() == "it" {
            found = true;
        }
    });
    found
}

fn walk_nodes(node: &SyntaxNode, visit: &mut impl FnMut(&SyntaxNode)) {
    visit(node);
    for child in node.children() {
        walk_nodes(&child, visit);
    }
}

fn parse_text_style(node: &SyntaxNode) -> Option<HeadingStyle> {
    let mut style = HeadingStyle::default();
    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;
    for child in args.children() {
        match child.kind() {
            SyntaxKind::Named => {
                let key = extract_named_key(&child).unwrap_or_default();
                let Some(value) = extract_named_value_node(&child) else {
                    continue;
                };
                match key.as_str() {
                    "size" => {
                        if let Some(text) = extract_literal_string(&value) {
                            style.size = Some(text);
                        }
                    }
                    "weight" => {
                        if let Some(text) = extract_literal_string(&value) {
                            style.bold |= is_bold_weight(&text);
                        }
                    }
                    "style" => {
                        if let Some(text) = extract_literal_string(&value) {
                            style.italic |= is_italic_style(&text);
                        }
                    }
                    _ => {}
                }
            }
            SyntaxKind::Numeric => {
                if style.size.is_none() {
                    style.size = Some(child.text().to_string());
                }
            }
            _ => {}
        }
    }
    if style.size.is_some() || style.bold || style.italic {
        Some(style)
    } else {
        None
    }
}

fn merge_heading_style(dest: &mut HeadingStyle, src: HeadingStyle) {
    if dest.size.is_none() {
        dest.size = src.size;
    }
    dest.bold |= src.bold;
    dest.italic |= src.italic;
    if dest.before.is_none() {
        dest.before = src.before;
    }
    if dest.after.is_none() {
        dest.after = src.after;
    }
}

fn is_bold_weight(value: &str) -> bool {
    let lowered = value.trim().trim_matches('"').to_lowercase();
    matches!(
        lowered.as_str(),
        "bold" | "semi-bold" | "semibold" | "extra-bold" | "extrabold" | "black" | "heavy"
    )
}

fn is_italic_style(value: &str) -> bool {
    let lowered = value.trim().trim_matches('"').to_lowercase();
    matches!(lowered.as_str(), "italic" | "oblique")
}

fn parse_length_to_pt(value: &str, font_size: &str) -> Option<f64> {
    let (num, unit) = split_number_unit(value)?;
    if unit == "pt" {
        return Some(num);
    }
    if unit == "em" {
        let (base, base_unit) = split_number_unit(font_size)?;
        if base_unit == "pt" {
            return Some(num * base);
        }
    }
    None
}

fn split_number_unit(value: &str) -> Option<(f64, String)> {
    let trimmed = value.trim();
    let mut number = String::new();
    let mut unit = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            number.push(ch);
        } else {
            unit.push(ch);
        }
    }
    if number.is_empty() {
        return None;
    }
    let num = number.parse::<f64>().ok()?;
    Some((num, unit.trim().to_string()))
}

fn map_text_size_option(value: &str) -> Option<&'static str> {
    let (num, unit) = split_number_unit(value)?;
    if unit != "pt" {
        return None;
    }
    if (num - 10.0).abs() < 0.2 {
        return Some("10pt");
    }
    if (num - 11.0).abs() < 0.2 {
        return Some("11pt");
    }
    if (num - 12.0).abs() < 0.2 {
        return Some("12pt");
    }
    None
}

fn map_paper_option(value: &str) -> Option<&'static str> {
    let normalized = value.trim().trim_matches('"').to_lowercase();
    match normalized.as_str() {
        "us-letter" | "letter" | "letterpaper" => Some("letterpaper"),
        "a4" | "a4paper" => Some("a4paper"),
        _ => None,
    }
}

fn parse_color_let(node: &SyntaxNode) -> Option<(String, String)> {
    let (name, value) = parse_let_value(node)?;
    let hex = extract_hex_color(&value)?;
    Some((name, hex))
}

fn parse_let_value(node: &SyntaxNode) -> Option<(String, SyntaxNode)> {
    let mut name: Option<String> = None;
    let mut value: Option<SyntaxNode> = None;
    let mut seen_eq = false;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Ident if name.is_none() => name = Some(child.text().to_string()),
            SyntaxKind::Eq => seen_eq = true,
            SyntaxKind::Space => {}
            _ => {
                if seen_eq {
                    value = Some(child.clone());
                    break;
                }
            }
        }
    }
    Some((name?, value?))
}

fn extract_hex_color(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => return extract_hex_from_text(&node.text()),
        SyntaxKind::FuncCall => {
            let name = get_func_call_name(node)?;
            if name != "rgb" {
                return None;
            }
            if let Some(hex) = node
                .children()
                .find(|c| c.kind() == SyntaxKind::Args)
                .and_then(|args| {
                    args.children()
                        .find(|c| c.kind() == SyntaxKind::Str)
                        .map(|s| extract_hex_from_text(&s.text()))
                })
                .flatten()
            {
                return Some(hex);
            }

            let mut nums = Vec::new();
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    if matches!(child.kind(), SyntaxKind::Numeric | SyntaxKind::Int | SyntaxKind::Float) {
                        if let Some(n) = parse_number(&child.text()) {
                            nums.push(n);
                        }
                    }
                }
            }
            if nums.len() == 3 {
                let mut vals = [nums[0], nums[1], nums[2]];
                if vals.iter().all(|v| *v <= 1.0) {
                    for v in &mut vals {
                        *v *= 255.0;
                    }
                }
                let hex = format!("{:02X}{:02X}{:02X}", clamp_byte(vals[0]), clamp_byte(vals[1]), clamp_byte(vals[2]));
                return Some(hex);
            }
        }
        _ => {}
    }
    None
}

fn clamp_byte(value: f64) -> u8 {
    if value.is_nan() {
        return 0;
    }
    let v = value.round().max(0.0).min(255.0);
    v as u8
}

fn extract_hex_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim().trim_matches('"');
    if let Some(pos) = trimmed.find('#') {
        let hex = &trimmed[pos + 1..];
        let mut out = String::new();
        for ch in hex.chars() {
            if !ch.is_ascii_hexdigit() {
                break;
            }
            if out.len() >= 6 {
                break;
            }
            out.push(ch);
        }
        if out.len() == 3 || out.len() == 6 {
            return Some(out.to_uppercase());
        }
    }
    None
}

fn parse_number(text: &str) -> Option<f64> {
    let mut num = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            num.push(ch);
        } else {
            break;
        }
    }
    if num.is_empty() {
        return None;
    }
    num.parse::<f64>().ok()
}

fn cite_uses_natbib(node: &SyntaxNode) -> bool {
    let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return false;
    };
    for child in args.children() {
        if child.kind() == SyntaxKind::Named {
            let key = extract_named_key(&child).unwrap_or_default();
            if matches!(
                key.as_str(),
                "style" | "form" | "mode" | "supplement" | "page" | "pages" | "note"
            ) {
                return true;
            }
        }
    }
    false
}

pub fn equation_numbering_enabled(hints: &PreambleHints) -> bool {
    if let Some(pattern) = hints.equation_numbering.as_deref() {
        let lowered = pattern.trim().to_lowercase();
        return lowered != "none" && lowered != "false";
    }
    false
}

pub fn is_two_column(hints: &PreambleHints) -> bool {
    hints.columns.unwrap_or(1) >= 2
}

fn is_theorem_like(name: &str) -> bool {
    matches!(
        name,
        "theorem"
            | "lemma"
            | "corollary"
            | "proposition"
            | "definition"
            | "example"
            | "remark"
            | "proof"
            | "claim"
            | "axiom"
    )
}

fn bibliography_style_needs_natbib(style: &str) -> bool {
    let lowered = style.trim().to_lowercase();
    lowered.contains("author")
        || lowered.contains("year")
        || lowered.contains("apa")
        || lowered.contains("chicago")
        || lowered.contains("harvard")
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

fn get_func_call_name(node: &SyntaxNode) -> Option<String> {
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

fn extract_named_key(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|c| c.kind() == SyntaxKind::Ident)
        .map(|n| n.text().to_string())
}

fn extract_named_value_node(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut seen_colon = false;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Colon => seen_colon = true,
            SyntaxKind::Space | SyntaxKind::Comma => {}
            _ if seen_colon => return Some(child.clone()),
            _ => {}
        }
    }
    None
}

fn extract_literal_string(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Numeric | SyntaxKind::Text | SyntaxKind::Ident => Some(node.text().to_string()),
        SyntaxKind::Bool => Some(node.text().to_string()),
        _ => None,
    }
}

fn parse_bool_literal(node: &SyntaxNode) -> Option<bool> {
    match node.text().trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_usize_literal(node: &SyntaxNode) -> Option<usize> {
    let raw = node.text().trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(val) = raw.parse::<usize>() {
        return Some(val);
    }
    if let Ok(val) = raw.parse::<f64>() {
        if val.is_finite() && val > 0.0 {
            return Some(val.round().clamp(1.0, 99.0) as usize);
        }
    }
    None
}

fn is_new_computer_modern(value: &str) -> bool {
    let lowered = value.trim().trim_matches('"').to_lowercase();
    lowered.contains("new computer modern")
}

fn escape_latex(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '$' => out.push_str("\\$"),
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            _ => out.push(ch),
        }
    }
    out
}
