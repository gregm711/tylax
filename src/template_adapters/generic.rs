use tylax_latex_backend::{render_document, LatexRenderOptions};
use tylax_typst_frontend::typst_to_ir;
use typst_syntax::{parse, SyntaxKind};

use crate::preamble_hints::{
    equation_numbering_enabled, extract_preamble_hints, is_two_column, parse_length_to_pt,
    render_article_preamble,
};
use crate::template_adapters::common::{
    collect_let_bindings, escape_latex, extract_array_strings, extract_named_args,
    extract_string_like, find_show_rule_with_prefix,
};

pub fn maybe_convert_template_with(input: &str) -> Option<String> {
    let root = parse(input);
    let (show_rule, _name) = find_show_rule_with_prefix(&root, "")?;
    let lets = collect_let_bindings(&root);
    let args = extract_named_args(&show_rule);
    if args.is_empty() {
        return None;
    }

    let mut meta: Vec<(String, String)> = Vec::new();
    for (key, value) in args {
        let rendered = match value.kind() {
            SyntaxKind::Array => {
                let values = extract_array_strings(&value, &lets);
                if values.is_empty() {
                    extract_string_like(&value, &lets)
                } else {
                    Some(values.join(", "))
                }
            }
            _ => extract_string_like(&value, &lets),
        };
        if let Some(text) = rendered {
            meta.push((key, text));
        }
    }

    if meta.is_empty() {
        return None;
    }

    let title = take_meta(&mut meta, &["title", "paper-title", "thesis-title"]);
    let subtitle = take_meta(&mut meta, &["subtitle", "sub-title"]);
    let author = take_meta(&mut meta, &["author", "authors", "name", "by"]);
    let date = take_meta(&mut meta, &["date", "year"]);
    let abstract_text = take_meta(&mut meta, &["abstract", "summary"]);
    let keywords = take_meta(
        &mut meta,
        &["keywords", "keyword", "key-words", "index-terms"],
    );

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
    let preamble = render_article_preamble(&hints);

    let mut out = String::new();
    out.push_str(&preamble);

    let has_title = title.is_some();
    let has_subtitle = subtitle.is_some();
    let has_author = author.is_some();
    let has_date = date.is_some();

    if has_title || has_subtitle || has_author || has_date {
        if let Some(title) = title.as_ref() {
            let mut title_line = escape_latex(&title);
            if let Some(subtitle) = subtitle.as_ref() {
                title_line.push_str("\\\\");
                title_line.push_str(&escape_latex(&subtitle));
            }
            out.push_str(&format!("\\title{{{}}}\n", title_line));
        } else if let Some(subtitle) = subtitle.as_ref() {
            out.push_str(&format!("\\title{{{}}}\n", escape_latex(&subtitle)));
        }
        if let Some(author) = author.as_ref() {
            out.push_str(&format!("\\author{{{}}}\n", escape_latex(&author)));
        }
        if let Some(date) = date.as_ref() {
            out.push_str(&format!("\\date{{{}}}\n", escape_latex(&date)));
        }
    }

    out.push_str("\\begin{document}\n\n");

    if has_title || has_author || has_date || has_subtitle {
        out.push_str("\\maketitle\n\n");
    }

    if let Some(abstract_text) = abstract_text {
        out.push_str("\\begin{abstract}\n");
        out.push_str(&escape_latex(&abstract_text));
        out.push_str("\n\\end{abstract}\n\n");
    }

    if let Some(keywords) = keywords {
        out.push_str("\\paragraph{Keywords} ");
        out.push_str(&escape_latex(&keywords));
        out.push_str("\n\n");
    }

    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    } else if !meta.is_empty() {
        out.push_str("\\section*{Metadata}\n");
        out.push_str("\\begin{description}\n");
        for (key, value) in meta {
            out.push_str(&format!(
                "\\item[{}] {}\n",
                escape_latex(&key),
                escape_latex(&value)
            ));
        }
        out.push_str("\\end{description}\n");
    }

    out.push_str("\\end{document}\n");
    Some(out)
}

fn take_meta(meta: &mut Vec<(String, String)>, keys: &[&str]) -> Option<String> {
    if let Some(pos) = meta
        .iter()
        .position(|(key, _)| keys.iter().any(|k| k.eq_ignore_ascii_case(key)))
    {
        Some(meta.remove(pos).1)
    } else {
        None
    }
}
