use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::{parse, SyntaxKind, SyntaxNode};

use crate::preamble_hints::{
    equation_number_within, equation_numbering_enabled, extract_preamble_hints, is_two_column,
    parse_length_to_pt, render_amsthm_definitions,
};

#[derive(Debug, Default)]
struct NewsletterMeta {
    title: Option<String>,
    edition: Option<String>,
    publication_info: Option<String>,
    hero_image: Option<String>,
    hero_caption: Option<String>,
}

pub fn maybe_convert_newsletter(input: &str) -> Option<String> {
    let root = parse(input);
    let show = find_show_with(&root, "newsletter.with")?;
    let meta = extract_meta(&show);

    let doc = typst_to_ir(input);
    let hints = extract_preamble_hints(input);
    let base_font_size_pt = hints
        .text_size
        .as_deref()
        .and_then(|size| parse_length_to_pt(size, "10pt"));
    let cite_command = hints.cite_command.clone().or_else(|| {
        if hints.uses_natbib {
            Some("citep".to_string())
        } else {
            None
        }
    });
    let body = render_document(
        &doc,
        LatexRenderOptions {
            full_document: false,
            number_equations: equation_numbering_enabled(&hints),
            two_column: is_two_column(&hints),
            inline_wide_tables: false,
            force_here: true,
            table_grid: false,
            table_style: tylax_latex_backend::TableStyle::Plain,
            table_caption_position: tylax_latex_backend::TableCaptionPosition::Bottom,
            bibliography_style_default: hints.bibliography_style.clone(),
            cite_command,
            base_font_size_pt,
            heading_numbering_none: hints.heading_numbering_none,
        },
    );

    let mut out = String::new();
    out.push_str("\\documentclass{article}\n");
    out.push_str("\\usepackage{graphicx}\n");
    out.push_str("\\usepackage{float}\n");
    out.push_str("\\usepackage{hyperref}\n");
    out.push_str("\\usepackage[table]{xcolor}\n");
    if hints.uses_amsthm {
        out.push_str("\\usepackage{amsthm}\n");
        out.push_str(&render_amsthm_definitions(&hints));
    }
    if hints.uses_natbib {
        out.push_str("\\usepackage{natbib}\n");
    }
    if let Some(within) = equation_number_within(&hints) {
        out.push_str(&format!("\\numberwithin{{equation}}{{{}}}\n", within));
    }
    out.push_str("\\begin{document}\n");

    if meta.title.is_some() || meta.edition.is_some() {
        out.push_str("\\begin{center}\n");
        if let Some(title) = meta.title.as_deref() {
            out.push_str("{\\LARGE\\bfseries ");
            out.push_str(&escape_latex(title));
            out.push_str("}\\\\\n");
        }
        if let Some(edition) = meta.edition.as_deref() {
            out.push_str(&escape_latex(edition));
            out.push_str("\n");
        }
        out.push_str("\\end{center}\n");
    }

    if let Some(image) = meta.hero_image.as_deref() {
        out.push_str("\\begin{figure}[h]\n\\centering\n");
        out.push_str("\\includegraphics[width=\\linewidth]{");
        out.push_str(&escape_latex(image));
        out.push_str("}\n");
        if let Some(caption) = meta.hero_caption.as_deref() {
            out.push_str("\\caption{");
            out.push_str(&escape_latex(caption));
            out.push_str("}\n");
        }
        out.push_str("\\end{figure}\n");
    }

    if let Some(info) = meta.publication_info.as_deref() {
        out.push_str("\\begin{center}\n");
        out.push_str("\\small ");
        out.push_str(&escape_latex(info));
        out.push_str("\n\\end{center}\n");
    }

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }

    out.push_str("\\end{document}\n");
    Some(out)
}

fn find_show_with(root: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    let mut stack = vec![root.clone()];
    while let Some(node) = stack.pop() {
        if node.kind() == SyntaxKind::ShowRule {
            if let Some(func) = node.children().find(|c| c.kind() == SyntaxKind::FuncCall) {
                if let Some(func_name) = func_call_name(&func) {
                    if func_name == name {
                        return Some(node);
                    }
                }
            }
        }
        for child in node.children() {
            stack.push(child.clone());
        }
    }
    None
}

fn extract_meta(show_rule: &SyntaxNode) -> NewsletterMeta {
    let mut meta = NewsletterMeta::default();
    let Some(func) = show_rule
        .children()
        .find(|c| c.kind() == SyntaxKind::FuncCall)
    else {
        return meta;
    };
    let Some(args) = func.children().find(|c| c.kind() == SyntaxKind::Args) else {
        return meta;
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
            "title" => meta.title = extract_string_like(&value),
            "edition" => meta.edition = extract_string_like(&value),
            "publication-info" => meta.publication_info = extract_string_like(&value),
            "hero-image" => {
                let (img, caption) = extract_hero_image(&value);
                meta.hero_image = img;
                meta.hero_caption = caption;
            }
            _ => {}
        }
    }
    meta
}

fn extract_hero_image(node: &SyntaxNode) -> (Option<String>, Option<String>) {
    if node.kind() != SyntaxKind::Dict {
        return (None, None);
    }
    let mut image = None;
    let mut caption = None;
    for child in node.children() {
        if child.kind() != SyntaxKind::Named {
            continue;
        }
        let key = extract_named_key(&child).unwrap_or_default();
        let Some(value) = extract_named_value_node(&child) else {
            continue;
        };
        match key.as_str() {
            "image" => image = extract_image_path(&value),
            "caption" => caption = extract_string_like(&value),
            _ => {}
        }
    }
    (image, caption)
}

fn extract_image_path(node: &SyntaxNode) -> Option<String> {
    if node.kind() == SyntaxKind::FuncCall {
        let name = func_call_name(node)?;
        if name == "image" {
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    if child.kind() == SyntaxKind::Str {
                        return Some(child.text().trim_matches('"').to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_string_like(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Text => Some(node.text().to_string()),
        SyntaxKind::Ident => Some(node.text().to_string()),
        SyntaxKind::ContentBlock | SyntaxKind::Markup => Some(extract_markup_text(node)),
        SyntaxKind::Array => {
            let values = extract_array_strings(node);
            if values.is_empty() {
                None
            } else {
                Some(values.join(", "))
            }
        }
        _ => Some(node_full_text(node)),
    }
}

fn extract_array_strings(node: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::LeftParen
            | SyntaxKind::RightParen
            | SyntaxKind::Comma
            | SyntaxKind::Space => continue,
            _ => {}
        }
        if let Some(value) = extract_string_like(&child) {
            out.push(value);
        }
    }
    out
}

fn extract_markup_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Text | SyntaxKind::Str => out.push_str(child.text().trim_matches('"')),
            SyntaxKind::Space => out.push(' '),
            _ => out.push_str(&extract_markup_text(&child)),
        }
    }
    out
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

fn node_full_text(node: &SyntaxNode) -> String {
    node.clone().into_text().to_string()
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
