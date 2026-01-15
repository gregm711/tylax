//! Typst AST to IR frontend.

use typst_syntax::{parse, SyntaxKind, SyntaxNode};
use tylax_ir::{
    Alignment, Block, BlockBlock, BoxBlock, Columns, Document, EnvironmentBlock, Figure,
    FigureContent, Grid, Image, Inline, ListKind, Loss, MathBlock, Table, TableCell,
};

mod preprocess;

pub fn typst_to_ir(input: &str) -> Document {
    let pre = preprocess::preprocess_typst(input);
    let root = parse(&pre.source);
    let mut losses = pre.losses;
    let blocks = collect_blocks(&root, &mut losses);
    Document::with_losses(blocks, losses)
}

struct PageBlock {
    blocks: Vec<Block>,
    numbering_none: bool,
}

fn collect_blocks(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current_inline: Vec<Inline> = Vec::new();

    let mut children: Vec<SyntaxNode> = Vec::new();
    for child in node.children() {
        if child.kind() == SyntaxKind::Markup {
            flatten_markup_children(&child, &mut children);
        } else {
            children.push(child.clone());
        }
    }
    let mut i = 0;
    while i < children.len() {
        let child = &children[i];
        match child.kind() {
            SyntaxKind::Import
            | SyntaxKind::ModuleImport
            | SyntaxKind::Include
            | SyntaxKind::ModuleInclude => {
                i += 1;
            }
            SyntaxKind::Heading => {
                flush_paragraph(&mut blocks, &mut current_inline);
                let level = count_heading_markers(&child) as u8;
                let content = collect_inlines(&child, losses);
                blocks.push(Block::Heading {
                    level,
                    content,
                    numbered: true,
                });
                i += 1;
            }
            SyntaxKind::Parbreak => {
                flush_paragraph(&mut blocks, &mut current_inline);
                i += 1;
            }
            SyntaxKind::ListItem => {
                flush_paragraph(&mut blocks, &mut current_inline);
                let (list_block, consumed) =
                    collect_list(&children[i..], ListKind::Unordered, losses);
                blocks.push(list_block);
                i += consumed;
            }
            SyntaxKind::EnumItem => {
                flush_paragraph(&mut blocks, &mut current_inline);
                let (list_block, consumed) = collect_list(&children[i..], ListKind::Ordered, losses);
                blocks.push(list_block);
                i += consumed;
            }
            SyntaxKind::Equation => {
                let mut prev = i as isize - 1;
                while prev >= 0 && matches!(children[prev as usize].kind(), SyntaxKind::Space) {
                    prev -= 1;
                }
                let mut next = i + 1;
                while next < children.len()
                    && matches!(children[next].kind(), SyntaxKind::Space)
                {
                    next += 1;
                }
                let next_kind = if next < children.len() {
                    Some(children[next].kind())
                } else {
                    None
                };

                let mut has_label = false;
                if next_kind == Some(SyntaxKind::Label) {
                    has_label = true;
                }

                let math = extract_math(&child);
                let needs_block = has_label
                    || math.as_deref().map_or(false, |expr| {
                        expr.contains('&') || expr.contains("\\\\") || expr.contains('\n')
                    });

                let has_inline_next = matches!(
                    next_kind,
                    Some(SyntaxKind::Text)
                        | Some(SyntaxKind::Str)
                        | Some(SyntaxKind::Strong)
                        | Some(SyntaxKind::Emph)
                        | Some(SyntaxKind::Link)
                        | Some(SyntaxKind::Code)
                        | Some(SyntaxKind::SmartQuote)
                        | Some(SyntaxKind::Shorthand)
                        | Some(SyntaxKind::Escape)
                        | Some(SyntaxKind::Linebreak)
                );
                let has_inline_content = current_inline.iter().any(|inline| match inline {
                    Inline::Text(text) => !text.trim().is_empty(),
                    Inline::LineBreak => true,
                    _ => true,
                });
                let inline_context = has_inline_content || has_inline_next;

                if inline_context && !needs_block {
                    if let Some(math) = math {
                        current_inline.push(Inline::Math(math));
                    }
                    i += 1;
                    continue;
                }

                flush_paragraph(&mut blocks, &mut current_inline);
                if let Some(math) = math {
                    let mut label: Option<String> = None;
                    let mut lookahead = i + 1;
                    while lookahead < children.len()
                        && matches!(children[lookahead].kind(), SyntaxKind::Space)
                    {
                        lookahead += 1;
                    }
                    if lookahead < children.len()
                        && children[lookahead].kind() == SyntaxKind::Label
                    {
                        if let Some(lab) = extract_label_text(&children[lookahead]) {
                            label = Some(lab);
                            i = lookahead;
                        }
                    }
                    blocks.push(Block::MathBlock(MathBlock { content: math, label }));
                }
                i += 1;
            }
            SyntaxKind::FuncCall => {
                if let Some(page) = maybe_page_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    if !blocks.is_empty() && !last_is_pagebreak(&blocks) {
                        blocks.push(Block::Paragraph(vec![Inline::RawLatex(
                            "\\newpage".to_string(),
                        )]));
                    }
                    if page.numbering_none {
                        blocks.push(Block::Paragraph(vec![Inline::RawLatex(
                            "\\thispagestyle{empty}".to_string(),
                        )]));
                    }
                    blocks.extend(page.blocks);
                    if has_more_content(&children, i + 1) && !last_is_pagebreak(&blocks) {
                        blocks.push(Block::Paragraph(vec![Inline::RawLatex(
                            "\\newpage".to_string(),
                        )]));
                    }
                } else if let Some(block) = maybe_pagebreak_block(&child) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_heading_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_environment_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_named_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_table_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_bibliography_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(mut block) = maybe_figure_block(&child, losses) {
                    if let Block::Figure(fig) = &mut block {
                        if fig.label.is_none() {
                            let mut lookahead = i + 1;
                            while lookahead < children.len()
                                && matches!(children[lookahead].kind(), SyntaxKind::Space)
                            {
                                lookahead += 1;
                            }
                            if lookahead < children.len()
                                && children[lookahead].kind() == SyntaxKind::Label
                            {
                                if let Some(lab) = extract_label_text(&children[lookahead]) {
                                    fig.label = Some(lab);
                                    i = lookahead;
                                }
                            }
                        }
                    }
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_image_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_quote_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_code_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_block_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_box_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_columns_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_grid_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_align_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_outline_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else if let Some(block) = maybe_vspace_block(&child, losses) {
                    flush_paragraph(&mut blocks, &mut current_inline);
                    blocks.push(block);
                } else {
                    current_inline.extend(collect_inlines(&child, losses));
                }
                i += 1;
            }
            _ => {
                current_inline.extend(collect_inlines(&child, losses));
                i += 1;
            }
        }
    }

    flush_paragraph(&mut blocks, &mut current_inline);
    blocks
}

fn last_is_pagebreak(blocks: &[Block]) -> bool {
    matches!(
        blocks.last(),
        Some(Block::Paragraph(inlines))
            if inlines.len() == 1
                && matches!(&inlines[0], Inline::RawLatex(raw) if raw.trim() == "\\newpage")
    )
}

fn has_more_content(children: &[SyntaxNode], start: usize) -> bool {
    let mut idx = start;
    while idx < children.len() {
        let kind = children[idx].kind();
        if matches!(kind, SyntaxKind::Space | SyntaxKind::Parbreak) {
            idx += 1;
            continue;
        }
        return true;
    }
    false
}

fn flatten_markup_children(node: &SyntaxNode, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        if child.kind() == SyntaxKind::Markup {
            flatten_markup_children(&child, out);
        } else {
            out.push(child.clone());
        }
    }
}

fn maybe_named_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    match func_name.as_str() {
        "article" => {
            let blocks = extract_content_blocks(node, losses);
            Some(Block::Block(BlockBlock { blocks }))
        }
        "important" => {
            let blocks = extract_content_blocks(node, losses);
            Some(Block::Block(BlockBlock { blocks }))
        }
        _ => None,
    }
}

fn maybe_environment_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    let env_name = match func_name.as_str() {
        "theorem" | "lemma" | "corollary" | "proposition" | "definition" | "example" | "remark"
        | "proof" | "claim" | "axiom" => Some(func_name),
        _ => None,
    }?;

    let mut title: Option<Vec<Inline>> = None;
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            if child.kind() == SyntaxKind::Named {
                let key = extract_named_key(&child).unwrap_or_default();
                if matches!(key.as_str(), "title" | "name" | "heading") {
                    if let Some(value) = extract_named_value_node(&child) {
                        if let Some(text) = parse_string_literal(&value) {
                            title = Some(vec![Inline::Text(text)]);
                        } else {
                            title = Some(collect_inlines(&value, losses));
                        }
                    }
                }
            }
        }
    }

    let blocks = extract_content_blocks(node, losses);
    Some(Block::Environment(EnvironmentBlock {
        name: env_name,
        title,
        blocks,
    }))
}

fn maybe_heading_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "heading" {
        return None;
    }

    let mut level: u8 = 1;
    let mut numbered = true;
    let mut content: Option<Vec<Inline>> = None;

    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::Named => {
                    let key = extract_named_key(&child).unwrap_or_default();
                    if let Some(value) = extract_named_value_node(&child) {
                        match key.as_str() {
                            "level" => {
                                if let Some(num) = parse_number_literal(&value) {
                                    level = num.round().clamp(1.0, 6.0) as u8;
                                } else if let Some(text) = parse_string_literal(&value) {
                                    if let Ok(num) = text.trim().parse::<u8>() {
                                        level = num.clamp(1, 6);
                                    }
                                }
                            }
                            "numbering" => {
                                if value.kind() == SyntaxKind::None {
                                    numbered = false;
                                } else if let Some(text) = parse_string_literal(&value) {
                                    if text.trim() == "none" {
                                        numbered = false;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                    content = Some(collect_inlines(&child, losses));
                }
                _ => {}
            }
        }
    }

    if content.is_none() {
        content = Some(extract_inline_content_from_call(node, losses));
    }

    Some(Block::Heading {
        level,
        content: content.unwrap_or_default(),
        numbered,
    })
}

fn collect_list(nodes: &[SyntaxNode], kind: ListKind, losses: &mut Vec<Loss>) -> (Block, usize) {
    let mut items: Vec<Vec<Block>> = Vec::new();
    let mut consumed = 0;

    let mut idx = 0;
    while idx < nodes.len() {
        let node = &nodes[idx];
        let is_item = match kind {
            ListKind::Unordered => node.kind() == SyntaxKind::ListItem,
            ListKind::Ordered => node.kind() == SyntaxKind::EnumItem,
        };
        if is_item {
            let item_blocks = collect_blocks(node, losses);
            items.push(if item_blocks.is_empty() {
                vec![Block::Paragraph(vec![])]
            } else {
                item_blocks
            });
            consumed += 1;
            idx += 1;
            continue;
        }

        if matches!(node.kind(), SyntaxKind::Space | SyntaxKind::Parbreak) {
            consumed += 1;
            idx += 1;
            continue;
        }

        break;
    }

    (Block::List { kind, items }, consumed)
}

fn collect_inlines(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Vec<Inline> {
    let mut out = Vec::new();
    match node.kind() {
        SyntaxKind::Import
        | SyntaxKind::ModuleImport
        | SyntaxKind::Include
        | SyntaxKind::ModuleInclude => {}
        SyntaxKind::Text | SyntaxKind::Str => {
            let text = node.text().to_string();
            if !text.is_empty() {
                out.push(Inline::Text(text));
            }
        }
        SyntaxKind::Space => {
            out.push(Inline::Text(" ".to_string()));
        }
        SyntaxKind::Parbreak | SyntaxKind::Linebreak => {
            out.push(Inline::LineBreak);
        }
        SyntaxKind::SmartQuote => {
            let text = map_smart_quote(node.text());
            if !text.is_empty() {
                out.push(Inline::Text(text));
            }
        }
        SyntaxKind::Shorthand => {
            if let Some(inline) = map_shorthand_inline(node.text()) {
                out.push(inline);
            }
        }
        SyntaxKind::Escape => {
            let text = decode_escape(node.text());
            if !text.is_empty() {
                out.push(Inline::Text(text));
            }
        }
        SyntaxKind::ListMarker | SyntaxKind::EnumMarker => {}
        SyntaxKind::Strong => {
            let mut inner = Vec::new();
            for child in node.children() {
                if child.kind() != SyntaxKind::Star {
                    inner.extend(collect_inlines(&child, losses));
                }
            }
            out.push(Inline::Strong(inner));
        }
        SyntaxKind::Emph => {
            let mut inner = Vec::new();
            for child in node.children() {
                if child.kind() != SyntaxKind::Underscore {
                    inner.extend(collect_inlines(&child, losses));
                }
            }
            out.push(Inline::Emph(inner));
        }
        SyntaxKind::Code => {
            let text = node.text().to_string();
            if !text.is_empty() {
                out.push(Inline::Code(text));
            }
        }
        SyntaxKind::Equation | SyntaxKind::Math => {
            if let Some(math) = extract_math(node) {
                out.push(Inline::Math(math));
            }
        }
        SyntaxKind::Link => {
            let url = node.text().to_string();
            out.push(Inline::Link {
                text: vec![Inline::Text(url.clone())],
                url,
            });
        }
        SyntaxKind::Ref => {
            let text = node_full_text(node);
            let label = text.trim_start_matches('@').to_string();
            if !label.is_empty() {
                if is_cross_ref_label(&label) {
                    out.push(Inline::Ref(label));
                } else {
                    out.push(Inline::Cite(label));
                }
            }
        }
        SyntaxKind::Label => {
            let text = node_full_text(node);
            let label = text.trim_start_matches('<').trim_end_matches('>').to_string();
            if !label.is_empty() {
                out.push(Inline::Label(label));
            }
        }
        SyntaxKind::FuncCall => {
            if let Some(inlines) = maybe_inline_func(node, losses) {
                out.extend(inlines);
            } else {
                for child in node.children() {
                    out.extend(collect_inlines(&child, losses));
                }
            }
        }
        SyntaxKind::FieldAccess => {
            let mut parts = Vec::new();
            for child in node.children() {
                if child.kind() == SyntaxKind::Ident {
                    parts.push(child.text().to_string());
                }
            }
            if let Some(inline) = sym_inline_from_parts(&parts) {
                out.push(inline);
            }
        }
        SyntaxKind::ContentBlock | SyntaxKind::Markup | SyntaxKind::CodeBlock => {
            for child in node.children() {
                out.extend(collect_inlines(&child, losses));
            }
        }
        _ => {
            for child in node.children() {
                out.extend(collect_inlines(&child, losses));
            }
        }
    }
    out
}

fn flush_paragraph(blocks: &mut Vec<Block>, current: &mut Vec<Inline>) {
    if current.is_empty() {
        return;
    }
    strip_trailing_bracket_artifact(current);
    blocks.push(Block::Paragraph(std::mem::take(current)));
}

fn strip_trailing_bracket_artifact(inlines: &mut Vec<Inline>) {
    loop {
        while let Some(last) = inlines.last() {
            match last {
                Inline::LineBreak => {
                    inlines.pop();
                }
                Inline::Text(text) if text.trim().is_empty() => {
                    inlines.pop();
                }
                _ => break,
            }
        }

        let mut open_brackets = 0usize;
        let mut close_brackets = 0usize;
        for inline in inlines.iter() {
            if let Inline::Text(text) = inline {
                for ch in text.chars() {
                    match ch {
                        '[' => open_brackets += 1,
                        ']' => close_brackets += 1,
                        _ => {}
                    }
                }
            }
        }

        let has_unbalanced_close = close_brackets > open_brackets;
        let mut removed = false;
        if let Some(Inline::Text(text)) = inlines.last_mut() {
            let trimmed = text.trim_end_matches(|ch: char| ch.is_whitespace());
            if trimmed == "]" && has_unbalanced_close {
                inlines.pop();
                removed = true;
            } else if trimmed.ends_with(']') && has_unbalanced_close {
                let mut chars = trimmed.chars().collect::<Vec<_>>();
                if chars.len() >= 2 && chars[chars.len() - 2].is_whitespace() {
                    chars.pop(); // remove ']'
                    let mut new: String = chars.into_iter().collect();
                    new = new.trim_end().to_string();
                    if new.is_empty() {
                        inlines.pop();
                    } else {
                        *text = new;
                    }
                    removed = true;
                }
            }
        }

        if !removed {
            break;
        }
    }
}

fn extract_math(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::Math {
            let text = node_full_text(&child);
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }
    let raw = node_full_text(node);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed.trim_matches('$').trim().to_string();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

fn extract_label_text(node: &SyntaxNode) -> Option<String> {
    let text = node_full_text(node);
    let label = text.trim().trim_start_matches('<').trim_end_matches('>').to_string();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

fn node_full_text(node: &SyntaxNode) -> String {
    node.clone().into_text().to_string()
}

fn count_heading_markers(node: &SyntaxNode) -> usize {
    for child in node.children() {
        if child.kind() == SyntaxKind::HeadingMarker {
            return child
                .text()
                .to_string()
                .chars()
                .filter(|&c| c == '=')
                .count();
        }
    }
    1
}

fn maybe_align_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let mut children = node.children();
    let func_name = children.next()?;
    if func_name.kind() != SyntaxKind::Ident {
        return None;
    }
    let name = func_name.text().to_string();
    if name != "align" && name != "center" {
        return None;
    }

    let args = children.next()?;
    let mut alignment = Alignment::Center;
    let mut content_blocks: Vec<Block> = Vec::new();

    if name == "align" {
        // Try to find alignment keyword in args
        for child in args.children() {
            if child.kind() == SyntaxKind::Ident {
                let text = child.text().to_string();
                alignment = match text.as_str() {
                    "left" | "start" => Alignment::Left,
                    "right" | "end" => Alignment::Right,
                    _ => Alignment::Center,
                };
            }
            if child.kind() == SyntaxKind::ContentBlock {
                content_blocks = collect_blocks(&child, losses);
            }
        }
    } else {
        // center(...) shorthand
        content_blocks = collect_blocks(&args, losses);
    }

    Some(Block::Align {
        alignment,
        blocks: if content_blocks.is_empty() {
            vec![Block::Paragraph(collect_inlines(&args, losses))]
        } else {
            content_blocks
        },
    })
}

fn maybe_table_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "table" {
        return None;
    }
    let table = parse_table_from_func_call(node, losses)?;
    Some(Block::Table(table))
}

fn maybe_figure_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "figure" {
        return None;
    }
    let mut caption: Option<Vec<Inline>> = None;
    let mut label: Option<String> = None;
    let mut placement: Option<String> = None;
    let mut content: Option<FigureContent> = None;

    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::Named => {
                    let key = extract_named_key(&child);
                    if let Some(key) = key {
                        if key == "caption" {
                            if let Some(value) = extract_named_value_node(&child) {
                                caption = Some(collect_inlines(&value, losses));
                            }
                        } else if key == "label" {
                            if let Some(value) = extract_named_value_node(&child) {
                                let text = value.text().to_string();
                                let cleaned = text.trim_matches(&['<', '>'][..]).to_string();
                                label = Some(cleaned);
                            }
                        } else if key == "placement" {
                            if let Some(value) = extract_named_value_node(&child) {
                                let text = value.text().trim_matches('"').to_string();
                                placement = Some(text);
                            }
                        }
                    }
                }
                SyntaxKind::FuncCall => {
                    if let Some(table) = parse_table_from_func_call(&child, losses) {
                        content = Some(FigureContent::Table(table));
                    } else if let Some(image) = parse_image_from_func_call(&child) {
                        content = Some(FigureContent::Image(image));
                    } else {
                        content = Some(FigureContent::Raw(collect_blocks(&child, losses)));
                    }
                }
                SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                    content = Some(extract_figure_content_from_node(&child, losses));
                }
                _ => {}
            }
        }
    }

    let content = content.unwrap_or(FigureContent::Raw(Vec::new()));
    Some(Block::Figure(Figure {
        content,
        caption,
        label,
        placement,
    }))
}

fn extract_figure_content_from_blocks(blocks: Vec<Block>) -> FigureContent {
    if blocks.len() == 1 {
        match blocks.into_iter().next().unwrap() {
            Block::Table(table) => FigureContent::Table(table),
            Block::Figure(fig) => {
                if fig.caption.is_none() && fig.label.is_none() {
                    fig.content
                } else {
                    FigureContent::Raw(vec![Block::Figure(fig)])
                }
            }
            other => FigureContent::Raw(vec![other]),
        }
    } else {
        FigureContent::Raw(blocks)
    }
}

fn extract_figure_content_from_node(node: &SyntaxNode, losses: &mut Vec<Loss>) -> FigureContent {
    if let Some(func) = find_descendant_func_call(node) {
        if let Some(table) = parse_table_from_func_call(&func, losses) {
            return FigureContent::Table(table);
        }
        if let Some(image) = parse_image_from_func_call(&func) {
            return FigureContent::Image(image);
        }
    }
    let blocks = collect_blocks(node, losses);
    extract_figure_content_from_blocks(blocks)
}

fn find_descendant_func_call(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut stack = vec![node.clone()];
    while let Some(current) = stack.pop() {
        if current.kind() == SyntaxKind::FuncCall {
            if let Some(name) = get_func_call_name(&current) {
                if name == "table" || name == "image" {
                    return Some(current);
                }
            }
        }
        for child in current.children() {
            stack.push(child.clone());
        }
    }
    None
}

fn maybe_image_block(node: &SyntaxNode, _losses: &mut Vec<Loss>) -> Option<Block> {
    let image = parse_image_from_func_call(node)?;
    let figure = Figure {
        content: FigureContent::Image(image),
        caption: None,
        label: None,
        placement: None,
    };
    Some(Block::Figure(figure))
}

fn maybe_bibliography_block(node: &SyntaxNode, _losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "bibliography" {
        return None;
    }
    let mut files: Vec<String> = Vec::new();
    let mut style: Option<String> = None;
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::Named => {
                    let key = extract_named_key(&child).unwrap_or_default();
                    if let Some(value) = extract_named_value_node(&child) {
                        if key == "style" {
                            style = Some(value.text().trim_matches('"').to_string());
                        } else if key == "file" || key == "files" {
                            collect_bibliography_files(&value, &mut files);
                        }
                    }
                }
                _ => {
                    collect_bibliography_files(&child, &mut files);
                }
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    let file = files.join(",");
    Some(Block::Bibliography { file, style })
}

fn collect_bibliography_files(node: &SyntaxNode, files: &mut Vec<String>) {
    match node.kind() {
        SyntaxKind::Named => {}
        SyntaxKind::Str => {
            files.push(node.text().trim_matches('"').to_string());
        }
        _ => {
            for child in node.children() {
                collect_bibliography_files(&child, files);
            }
        }
    }
}

fn parse_image_from_func_call(node: &SyntaxNode) -> Option<Image> {
    let func_name = get_func_call_name(node)?;
    if func_name != "image" {
        return None;
    }
    let mut path: Option<String> = None;
    let mut width: Option<String> = None;
    let mut height: Option<String> = None;
    let mut fit: Option<String> = None;
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::Str => {
                    if path.is_none() {
                        path = Some(child.text().trim_matches('"').to_string());
                    }
                }
                SyntaxKind::Named => {
                    let key = extract_named_key(&child).unwrap_or_default();
                    if let Some(value) = extract_named_value_node(&child) {
                        let text = value.text().trim_matches('"').to_string();
                        match key.as_str() {
                            "width" => width = Some(text),
                            "height" => height = Some(text),
                            "fit" => fit = Some(text),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let path = path?;
    Some(Image {
        path,
        width,
        height,
        fit,
    })
}

fn maybe_quote_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "quote" {
        return None;
    }
    let mut blocks = Vec::new();
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            if child.kind() == SyntaxKind::ContentBlock || child.kind() == SyntaxKind::Markup {
                blocks = collect_blocks(&child, losses);
                break;
            }
        }
    }
    Some(Block::Quote(blocks))
}

fn maybe_block_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "block" {
        return None;
    }
    let blocks = extract_content_blocks(node, losses);
    Some(Block::Block(BlockBlock { blocks }))
}

fn maybe_box_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "box" {
        return None;
    }
    let blocks = extract_content_blocks(node, losses);
    Some(Block::Box(BoxBlock { blocks }))
}

fn maybe_columns_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "columns" {
        return None;
    }
    let mut columns = 2usize;
    let mut blocks = Vec::new();

    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::Named => {
                    let key = extract_named_key(&child).unwrap_or_default();
                    if key == "columns" {
                        if let Some(value) = extract_named_value_node(&child) {
                            if let Ok(n) = value.text().trim().parse::<usize>() {
                                columns = n.max(1);
                            }
                        }
                    }
                }
                SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                    blocks.push(Block::Paragraph(collect_inlines(&child, losses)));
                }
                _ => {}
            }
        }
    }
    Some(Block::Columns(Columns { columns, blocks }))
}

fn maybe_grid_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "grid" {
        return None;
    }
    let mut columns = 2usize;
    let mut cells: Vec<Vec<Block>> = Vec::new();
    let mut gutter: Option<String> = None;
    let mut row_gutter: Option<String> = None;
    let mut column_gutter: Option<String> = None;

    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::Named => {
                    let key = extract_named_key(&child).unwrap_or_default();
                    if key == "columns" {
                        if let Some(value) = extract_named_value_node(&child) {
                            if let Some(n) = infer_table_columns(value.text().as_ref()) {
                                columns = n.max(1);
                            }
                        }
                    } else if key == "gutter" {
                        if let Some(value) = extract_named_value_node(&child) {
                            gutter = Some(node_full_text(&value));
                        }
                    } else if key == "row-gutter" {
                        if let Some(value) = extract_named_value_node(&child) {
                            row_gutter = Some(node_full_text(&value));
                        }
                    } else if key == "column-gutter" {
                        if let Some(value) = extract_named_value_node(&child) {
                            column_gutter = Some(node_full_text(&value));
                        }
                    }
                }
                SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                    cells.push(collect_blocks(&child, losses));
                }
                _ => {}
            }
        }
    }

    Some(Block::Grid(Grid {
        columns,
        cells,
        gutter,
        row_gutter,
        column_gutter,
    }))
}

fn maybe_code_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "raw" {
        return None;
    }
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            if child.kind() == SyntaxKind::Str || child.kind() == SyntaxKind::Text {
                let text = child.text().to_string();
                return Some(Block::CodeBlock(text.trim_matches('"').to_string()));
            }
            if child.kind() == SyntaxKind::ContentBlock {
                let text = node_full_text(&child);
                return Some(Block::CodeBlock(text));
            }
        }
    }
    losses.push(Loss::new(
        "raw",
        "raw block without simple string content not supported",
    ));
    None
}

fn parse_table_from_func_call(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Table> {
    let func_name = get_func_call_name(node)?;
    if func_name != "table" {
        return None;
    }

    let mut columns: Option<usize> = None;
    let mut align: Option<Vec<Alignment>> = None;
    let mut caption: Option<Vec<Inline>> = None;
    let mut cells: Vec<TableCell> = Vec::new();
    let mut stroke: Option<String> = None;
    let mut fill: Option<String> = None;
    let mut inset: Option<String> = None;

    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;
    for child in args.children() {
        match child.kind() {
            SyntaxKind::Named => {
                let key = extract_named_key(&child).unwrap_or_default();
                if let Some(value) = extract_named_value_node(&child) {
                    let value_text = value.text().to_string();
                    if key == "columns" {
                        columns = infer_table_columns(&value_text);
                    } else if key == "align" {
                        align = Some(parse_typst_align(&value_text));
                    } else if key == "caption" {
                        caption = Some(collect_inlines(&value, losses));
                    } else if key == "stroke" {
                        stroke = Some(node_full_text(&value));
                    } else if key == "fill" {
                        fill = Some(node_full_text(&value));
                    } else if key == "inset" {
                        inset = Some(node_full_text(&value));
                    }
                }
            }
            SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                let content = collect_inlines(&child, losses);
                cells.push(TableCell {
                    content,
                    colspan: 1,
                    rowspan: 1,
                    align: None,
                    is_header: false,
                    fill: None,
                    stroke: None,
                    inset: None,
                });
            }
            SyntaxKind::FuncCall => {
                // If it's table.cell(...) just capture its content block as a cell.
                if let Some(header_cells) = extract_table_header_cells(&child, losses) {
                    cells.extend(header_cells);
                } else if let Some(cell) = extract_cell_from_table_cell(&child, losses) {
                    cells.push(cell);
                } else {
                    let content = collect_inlines(&child, losses);
                    cells.push(TableCell {
                        content,
                        colspan: 1,
                        rowspan: 1,
                        align: None,
                        is_header: false,
                        fill: None,
                        stroke: None,
                        inset: None,
                    });
                }
            }
            _ => {}
        }
    }

    let columns = columns.unwrap_or_else(|| infer_columns_from_cells(cells.len()));
    Some(Table {
        columns: columns.max(1),
        cells,
        align,
        caption,
        stroke,
        fill,
        inset,
    })
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

fn extract_cell_from_table_cell(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<TableCell> {
    let name = get_func_call_name(node)?;
    if !name.contains("cell") {
        return None;
    }
    let mut colspan = 1usize;
    let mut rowspan = 1usize;
    let mut align: Option<Alignment> = None;
    let mut is_header = false;
    let mut content: Option<Vec<Inline>> = None;
    let mut fill: Option<String> = None;
    let mut stroke: Option<String> = None;
    let mut inset: Option<String> = None;

    for child in node.children() {
        match child.kind() {
            SyntaxKind::Args => {
                for arg in child.children() {
                    match arg.kind() {
                        SyntaxKind::Named => {
                            let key = extract_named_key(&arg).unwrap_or_default();
                            if let Some(value) = extract_named_value_node(&arg) {
                                let text = value.text().to_string();
                                if key == "colspan" {
                                    if let Ok(n) = text.trim().parse::<usize>() {
                                        colspan = n.max(1);
                                    }
                                } else if key == "rowspan" {
                                    if let Ok(n) = text.trim().parse::<usize>() {
                                        rowspan = n.max(1);
                                    }
                                } else if key == "align" {
                                    let parsed = parse_typst_align(&text);
                                    if let Some(first) = parsed.first() {
                                        align = Some(*first);
                                    }
                                } else if key == "header" {
                                    is_header = text.contains("true");
                                } else if key == "stroke" {
                                    stroke = Some(node_full_text(&value));
                                } else if key == "fill" {
                                    fill = Some(node_full_text(&value));
                                } else if key == "inset" {
                                    inset = Some(node_full_text(&value));
                                }
                            }
                        }
                        SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                            content = Some(collect_inlines(&arg, losses));
                        }
                        _ => {}
                    }
                }
            }
            SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                content = Some(collect_inlines(&child, losses));
            }
            _ => {}
        }
    }
    let content = content.unwrap_or_default();
    Some(TableCell {
        content,
        colspan,
        rowspan,
        align,
        is_header,
        fill,
        stroke,
        inset,
    })
}

fn extract_table_header_cells(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Vec<TableCell>> {
    let name = get_func_call_name(node)?;
    if name != "table.header" {
        return None;
    }
    let mut cells = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::Args => {
                for arg in child.children() {
                    if matches!(arg.kind(), SyntaxKind::ContentBlock | SyntaxKind::Markup) {
                        let content = collect_inlines(&arg, losses);
                        cells.push(TableCell {
                            content,
                            colspan: 1,
                            rowspan: 1,
                            align: None,
                            is_header: true,
                            fill: None,
                            stroke: None,
                            inset: None,
                        });
                    }
                }
            }
            SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                let content = collect_inlines(&child, losses);
                cells.push(TableCell {
                    content,
                    colspan: 1,
                    rowspan: 1,
                    align: None,
                    is_header: true,
                    fill: None,
                    stroke: None,
                    inset: None,
                });
            }
            _ => {}
        }
    }
    if cells.is_empty() {
        return None;
    }
    Some(cells)
}

fn infer_columns_from_cells(cell_count: usize) -> usize {
    if cell_count == 0 {
        return 1;
    }
    let root = (cell_count as f64).sqrt().ceil() as usize;
    root.max(1)
}

fn infer_table_columns(value: &str) -> Option<usize> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Ok(n) = v.parse::<usize>() {
        return Some(n.max(1));
    }
    let inner = v
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    if inner.is_empty() {
        return None;
    }
    let commas = inner.matches(',').count();
    if commas > 0 {
        return Some(commas + 1);
    }
    let auto_count = inner.matches("auto").count();
    if auto_count > 0 {
        return Some(auto_count);
    }
    let fr_count = inner.matches("fr").count();
    if fr_count > 0 {
        return Some(fr_count);
    }
    Some(1)
}

fn parse_typst_align(value: &str) -> Vec<Alignment> {
    let inner = value
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|s| {
            let trimmed = s.trim();
            let lowered = trimmed.to_lowercase();
            if lowered.contains("left") || lowered.contains("start") {
                Alignment::Left
            } else if lowered.contains("right") || lowered.contains("end") {
                Alignment::Right
            } else if lowered.contains("center") {
                Alignment::Center
            } else {
                Alignment::Center
            }
        })
        .collect()
}

fn maybe_outline_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "outline" {
        return None;
    }
    let mut title: Option<Vec<Inline>> = None;
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            if child.kind() == SyntaxKind::Named {
                let key = extract_named_key(&child).unwrap_or_default();
                if key == "title" {
                    if let Some(value) = extract_named_value_node(&child) {
                        title = Some(collect_inlines(&value, losses));
                    }
                } else if key == "target" {
                    losses.push(Loss::new(
                        "outline",
                        "outline target not supported in IR pipeline",
                    ));
                }
            }
        }
    }
    Some(Block::Outline { title })
}

fn maybe_page_block(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<PageBlock> {
    let func_name = get_func_call_name(node)?;
    if func_name != "page" {
        return None;
    }
    let mut numbering_none = false;
    let mut content_blocks: Vec<Block> = Vec::new();

    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            if child.kind() == SyntaxKind::Named {
                let key = extract_named_key(&child).unwrap_or_default();
                if key == "numbering" {
                    if let Some(value) = extract_named_value_node(&child) {
                        if value.kind() == SyntaxKind::None {
                            numbering_none = true;
                        } else if let Some(text) = parse_string_literal(&value) {
                            if text.trim() == "none" {
                                numbering_none = true;
                            }
                        }
                    }
                }
            }
            if child.kind() == SyntaxKind::ContentBlock || child.kind() == SyntaxKind::Markup {
                content_blocks = collect_blocks(&child, losses);
            }
        }
    }

    if content_blocks.is_empty() {
        content_blocks = vec![Block::Paragraph(collect_inlines(node, losses))];
    }

    Some(PageBlock {
        blocks: content_blocks,
        numbering_none,
    })
}

fn maybe_pagebreak_block(node: &SyntaxNode) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "pagebreak" {
        return None;
    }
    Some(Block::Paragraph(vec![Inline::RawLatex(
        "\\newpage".to_string(),
    )]))
}

fn maybe_vspace_block(node: &SyntaxNode, _losses: &mut Vec<Loss>) -> Option<Block> {
    let func_name = get_func_call_name(node)?;
    if func_name != "v" {
        return None;
    }
    let size = first_arg_text(node)?;
    Some(Block::VSpace(size))
}

fn maybe_inline_func(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Option<Vec<Inline>> {
    let func_name = get_func_call_name(node)?;
    match func_name.as_str() {
        "important" => {
            let content = extract_inline_content_from_call(node, losses);
            if !content.is_empty() {
                return Some(content);
            }
        }
        "cite" => {
            let mut keys: Vec<String> = Vec::new();
            let mut style: Option<String> = None;
            let mut pre_note: Option<String> = None;
            let mut post_note: Option<String> = None;
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    match child.kind() {
                        SyntaxKind::Named => {
                            let key = extract_named_key(&child).unwrap_or_default();
                            if let Some(value) = extract_named_value_node(&child) {
                                let val = parse_string_literal(&value)
                                    .unwrap_or_else(|| value.text().to_string());
                                let cleaned = val.trim_matches('"').to_string();
                                if matches!(key.as_str(), "style" | "form" | "mode") {
                                    style = Some(cleaned);
                                } else if key == "supplement" {
                                    if !cleaned.is_empty() {
                                        pre_note = Some(cleaned);
                                    }
                                } else if key == "page" {
                                    if !cleaned.is_empty() {
                                        let note = format_page_note(&cleaned, false);
                                        push_note(&mut post_note, &note);
                                    }
                                } else if key == "pages" {
                                    if !cleaned.is_empty() {
                                        let note = format_page_note(&cleaned, true);
                                        push_note(&mut post_note, &note);
                                    }
                                } else if key == "note" {
                                    if !cleaned.is_empty() {
                                        push_note(&mut post_note, &cleaned);
                                    }
                                }
                            }
                        }
                        _ => {
                            collect_cite_keys(&child, &mut keys);
                        }
                    }
                }
            }

            if !keys.is_empty() {
                let command = if let Some(style) = style.as_deref() {
                    let lowered = style.to_lowercase();
                    if lowered.contains("author") || lowered.contains("text") {
                        "citet"
                    } else if lowered.contains("year") {
                        if pre_note.is_some() || post_note.is_some() {
                            "citep"
                        } else {
                            "citeyearpar"
                        }
                    } else if lowered.contains("paren") {
                        "citep"
                    } else {
                        "cite"
                    }
                } else if pre_note.is_some() || post_note.is_some() {
                    "citep"
                } else {
                    "cite"
                };

                if pre_note.is_some() || post_note.is_some() || command != "cite" {
                    let cite = match (pre_note.as_deref(), post_note.as_deref()) {
                        (Some(pre), Some(post)) => {
                            format!("\\{}[{}][{}]{{{}}}", command, pre, post, keys.join(","))
                        }
                        (Some(note), None) | (None, Some(note)) => {
                            format!("\\{}[{}]{{{}}}", command, note, keys.join(","))
                        }
                        (None, None) => format!("\\{}{{{}}}", command, keys.join(",")),
                    };
                    return Some(vec![Inline::RawLatex(cite)]);
                }

                return Some(vec![Inline::Cite(keys.join(","))]);
            }
        }
        "footnote" => {
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    if child.kind() == SyntaxKind::ContentBlock || child.kind() == SyntaxKind::Markup
                    {
                        return Some(vec![Inline::Footnote(collect_inlines(&child, losses))]);
                    }
                }
            }
        }
        "ref" => {
            let mut supplement: Option<Vec<Inline>> = None;
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    if child.kind() == SyntaxKind::Named {
                        let key = extract_named_key(&child).unwrap_or_default();
                        if key == "supplement" {
                            if let Some(value) = extract_named_value_node(&child) {
                                let inlines = collect_inlines(&value, losses);
                                if !inlines.is_empty() {
                                    supplement = Some(inlines);
                                }
                            }
                        }
                    }
                }
                if let Some(label) = extract_ref_label(&args) {
                    if let Some(mut supplement) = supplement {
                        supplement.push(Inline::RawLatex("\\nobreakspace{}".to_string()));
                        supplement.push(Inline::Ref(label));
                        return Some(supplement);
                    }
                    return Some(vec![Inline::Ref(label)]);
                }
            }
        }
        "label" => {
            if let Some(arg) = first_arg_string(node) {
                return Some(vec![Inline::Label(arg)]);
            }
        }
        "link" => {
            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                let mut url: Option<String> = None;
                let mut text: Option<Vec<Inline>> = None;
                for child in args.children() {
                    if child.kind() == SyntaxKind::Str {
                        url = Some(child.text().trim_matches('"').to_string());
                    } else if child.kind() == SyntaxKind::ContentBlock
                        || child.kind() == SyntaxKind::Markup
                    {
                        text = Some(collect_inlines(&child, losses));
                    }
                }
                if let Some(url) = url {
                    return Some(vec![Inline::Link {
                        text: text.unwrap_or_else(|| vec![Inline::Text(url.clone())]),
                        url,
                    }]);
                }
            }
        }
        "text" => {
            let mut bold = false;
            let mut italic = false;
            let mut color: Option<String> = None;
            let mut size: Option<String> = None;
            let mut content: Option<Vec<Inline>> = None;

            if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
                for child in args.children() {
                    match child.kind() {
                        SyntaxKind::Named => {
                            let key = extract_named_key(&child).unwrap_or_default();
                            if let Some(value) = extract_named_value_node(&child) {
                                match key.as_str() {
                                    "weight" => {
                                        if let Some(value) = parse_string_literal(&value) {
                                            bold |= is_bold_weight(&value);
                                        } else if let Some(num) = parse_number_literal(&value) {
                                            bold |= num >= 600.0;
                                        }
                                    }
                                    "style" => {
                                        if let Some(value) = parse_string_literal(&value) {
                                            italic |= is_italic_style(&value);
                                        }
                                    }
                                    "fill" => {
                                        if let Some(parsed) = parse_color_value(&value) {
                                            color = Some(parsed);
                                        }
                                    }
                                    "size" => {
                                        if let Some(parsed) = parse_string_literal(&value) {
                                            size = Some(parsed);
                                        } else if let Some(num) = parse_number_literal(&value) {
                                            size = Some(format!("{num}pt"));
                                        } else {
                                            let raw = value.text().trim().trim_matches('"');
                                            if !raw.is_empty() {
                                                size = Some(raw.to_string());
                                            }
                                        }
                                    }
                                    "italic" => {
                                        if let Some(value) = parse_bool_literal(&value) {
                                            italic |= value;
                                        }
                                    }
                                    "bold" | "strong" => {
                                        if let Some(value) = parse_bool_literal(&value) {
                                            bold |= value;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                            content = Some(collect_inlines(&child, losses));
                        }
                        SyntaxKind::Str => {
                            if content.is_none() {
                                content = Some(vec![Inline::Text(
                                    child.text().trim_matches('"').to_string(),
                                )]);
                            }
                        }
                        _ => {}
                    }
                }
            }

            if content.is_none() {
                content = Some(extract_inline_content_from_call(node, losses));
            }

            let content = content.unwrap_or_default();
            let mut wrapped = content;
            if italic {
                wrapped = vec![Inline::Emph(wrapped)];
            }
            if bold {
                wrapped = vec![Inline::Strong(wrapped)];
            }
            if let Some(color) = color {
                wrapped = vec![Inline::Color { color, content: wrapped }];
            }
            if let Some(size) = size {
                wrapped = vec![Inline::Size { size, content: wrapped }];
            }
            return Some(wrapped);
        }
        "super" => {
            let content = extract_inline_content_from_call(node, losses);
            return Some(vec![Inline::Superscript(content)]);
        }
        "sub" => {
            let content = extract_inline_content_from_call(node, losses);
            return Some(vec![Inline::Subscript(content)]);
        }
        _ if func_name.starts_with("sym.") => {
            let parts: Vec<String> = func_name.split('.').map(|s| s.to_string()).collect();
            if let Some(inline) = sym_inline_from_parts(&parts) {
                return Some(vec![inline]);
            }
        }
        _ => {}
    }
    None
}

fn extract_inline_content_from_call(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Vec<Inline> {
    for child in node.children() {
        if matches!(child.kind(), SyntaxKind::ContentBlock | SyntaxKind::Markup) {
            return collect_inlines(&child, losses);
        }
    }
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            match child.kind() {
                SyntaxKind::ContentBlock | SyntaxKind::Markup => {
                    return collect_inlines(&child, losses);
                }
                SyntaxKind::Str => {
                    return vec![Inline::Text(child.text().trim_matches('"').to_string())];
                }
                SyntaxKind::Text => {
                    return vec![Inline::Text(child.text().to_string())];
                }
                _ => {}
            }
        }
    }
    Vec::new()
}

fn map_smart_quote(raw: &str) -> String {
    match raw {
        "\u{201C}" | "\u{201D}" => "\"".to_string(),
        "\u{2018}" | "\u{2019}" => "'".to_string(),
        _ => raw.to_string(),
    }
}

fn map_shorthand_inline(raw: &str) -> Option<Inline> {
    match raw {
        "~" => Some(Inline::RawLatex("\\nobreakspace{}".to_string())),
        "-?" => Some(Inline::RawLatex("\\-".to_string())),
        "--" => Some(Inline::Text("--".to_string())),
        "---" => Some(Inline::Text("---".to_string())),
        "..." | "\u{2026}" => Some(Inline::Text("...".to_string())),
        _ => {
            if raw.is_empty() {
                None
            } else {
                Some(Inline::Text(raw.to_string()))
            }
        }
    }
}

fn decode_escape(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(body) = trimmed.strip_prefix("\\u{") {
        if let Some(end) = body.find('}') {
            let hex = &body[..end];
            if let Ok(code) = u32::from_str_radix(hex, 16) {
                if let Some(ch) = char::from_u32(code) {
                    return ch.to_string();
                }
            }
        }
    }
    if let Some(rest) = trimmed.strip_prefix('\\') {
        return rest.to_string();
    }
    trimmed.to_string()
}

fn parse_color_value(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => return Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Ident | SyntaxKind::Text => return Some(node.text().to_string()),
        SyntaxKind::FuncCall => {
            let name = get_func_call_name(node)?;
            if name == "rgb" {
                for child in node.children() {
                    if child.kind() == SyntaxKind::Args {
                        for arg in child.children() {
                            if arg.kind() == SyntaxKind::Str {
                                return Some(arg.text().trim_matches('"').to_string());
                            }
                        }
                    }
                }
                // Fallback to raw text if rgb(...) uses numeric values.
                return Some(node.text().to_string());
            }
        }
        _ => {}
    }
    None
}

fn parse_string_literal(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::Str => Some(node.text().trim_matches('"').to_string()),
        SyntaxKind::Ident | SyntaxKind::Text => Some(node.text().to_string()),
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

fn parse_number_literal(node: &SyntaxNode) -> Option<f64> {
    node.text().trim().parse::<f64>().ok()
}

fn is_bold_weight(value: &str) -> bool {
    let lowered = value.trim().trim_matches('"').to_lowercase();
    if matches!(
        lowered.as_str(),
        "bold" | "semi-bold" | "semibold" | "extra-bold" | "extrabold" | "black" | "heavy"
    ) {
        return true;
    }
    if let Ok(num) = lowered.parse::<f64>() {
        return num >= 600.0;
    }
    false
}

fn is_italic_style(value: &str) -> bool {
    let lowered = value.trim().trim_matches('"').to_lowercase();
    matches!(lowered.as_str(), "italic" | "oblique")
}

fn sym_inline_from_parts(parts: &[String]) -> Option<Inline> {
    let key = parts.join(".");
    match key.as_str() {
        "sym.ast" => Some(Inline::Text("*".to_string())),
        "sym.dagger" => Some(Inline::RawLatex("\\textdagger{}".to_string())),
        "sym.ddagger" => Some(Inline::RawLatex("\\textdaggerdbl{}".to_string())),
        "sym.degree" => Some(Inline::RawLatex("\\textdegree{}".to_string())),
        "sym.bullet" => Some(Inline::RawLatex("\\textbullet{}".to_string())),
        "sym.space.nobreak" => Some(Inline::RawLatex("\\nobreakspace{}".to_string())),
        "sym.wj" => Some(Inline::RawLatex("\\nobreak".to_string())),
        _ => None,
    }
}

fn is_cross_ref_label(label: &str) -> bool {
    let lower = label.to_lowercase();
    for prefix in [
        "fig:", "tab:", "sec:", "eq:", "lst:", "alg:", "thm:", "lemma:", "lem:", "prop:",
        "def:", "cor:", "ex:", "remark:", "rem:", "app:", "appendix:",
    ] {
        if lower.starts_with(prefix) {
            return true;
        }
    }
    false
}

fn first_arg_string(node: &SyntaxNode) -> Option<String> {
    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;
    for child in args.children() {
        if child.kind() == SyntaxKind::Str || child.kind() == SyntaxKind::Text {
            return Some(child.text().trim_matches('"').to_string());
        }
    }
    None
}

fn first_arg_text(node: &SyntaxNode) -> Option<String> {
    let args = node.children().find(|c| c.kind() == SyntaxKind::Args)?;
    for child in args.children() {
        if matches!(
            child.kind(),
            SyntaxKind::Str | SyntaxKind::Text | SyntaxKind::Numeric | SyntaxKind::Ident
        ) {
            return Some(child.text().trim_matches('"').to_string());
        }
    }
    None
}

fn extract_ref_label(args: &SyntaxNode) -> Option<String> {
    for child in args.children() {
        if child.kind() == SyntaxKind::Named {
            continue;
        }
        match child.kind() {
            SyntaxKind::Label | SyntaxKind::Ref => {
                let text = node_full_text(&child);
                let cleaned = text
                    .trim()
                    .trim_start_matches('@')
                    .trim_start_matches('<')
                    .trim_end_matches('>');
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
            SyntaxKind::Str | SyntaxKind::Text | SyntaxKind::Ident => {
                let text = child.text().trim_matches('"').to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
            _ => {}
        }
    }
    None
}

fn format_page_note(raw: &str, plural: bool) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lowered = trimmed.to_lowercase();
    if lowered.starts_with('p') {
        return trimmed.to_string();
    }
    let prefix = if plural { "pp.~" } else { "p.~" };
    format!("{}{}", prefix, trimmed)
}

fn push_note(note: &mut Option<String>, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    match note {
        Some(existing) => {
            if !existing.is_empty() {
                existing.push_str(", ");
            }
            existing.push_str(value.trim());
        }
        None => {
            *note = Some(value.trim().to_string());
        }
    }
}

fn collect_cite_keys(node: &SyntaxNode, keys: &mut Vec<String>) {
    match node.kind() {
        SyntaxKind::Ref => {
            let text = node_full_text(node);
            push_cite_key(keys, &text);
        }
        SyntaxKind::Label => {
            let text = node_full_text(node);
            push_cite_key(keys, &text);
        }
        SyntaxKind::Str | SyntaxKind::Text | SyntaxKind::Ident => {
            let text = node.text().to_string();
            push_cite_key(keys, &text);
        }
        SyntaxKind::Named => {
            // ignore named args (handled separately)
        }
        _ => {
            for child in node.children() {
                collect_cite_keys(&child, keys);
            }
        }
    }
}

fn push_cite_key(keys: &mut Vec<String>, raw: &str) {
    let cleaned = raw
        .trim()
        .trim_matches('"')
        .trim_start_matches('@')
        .trim_start_matches('<')
        .trim_end_matches('>');
    for part in cleaned.split(',') {
        let key = part.trim();
        if key.is_empty() {
            continue;
        }
        if !keys.iter().any(|k| k == key) {
            keys.push(key.to_string());
        }
    }
}

fn extract_content_blocks(node: &SyntaxNode, losses: &mut Vec<Loss>) -> Vec<Block> {
    if let Some(args) = node.children().find(|c| c.kind() == SyntaxKind::Args) {
        for child in args.children() {
            if child.kind() == SyntaxKind::ContentBlock || child.kind() == SyntaxKind::Markup {
                return collect_blocks(&child, losses);
            }
        }
    }
    Vec::new()
}
