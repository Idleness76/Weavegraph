use std::collections::HashSet;

use scraper::{node::Node, ElementRef, Html, Selector};

use crate::semantic_chunking::config::HtmlPreprocessConfig;

const HEADING_TAGS: &[(&str, usize)] = &[
    ("h1", 1),
    ("h2", 2),
    ("h3", 3),
    ("h4", 4),
    ("h5", 5),
    ("h6", 6),
];

const BLOCK_TAGS: &[&str] = &[
    "p",
    "li",
    "blockquote",
    "pre",
    "code",
    "td",
    "th",
    "figcaption",
    "summary",
    "article",
    "section",
];

const PRESERVE_WHITESPACE_TAGS: &[&str] = &["pre", "code"];

#[derive(Clone, Debug, Default)]
pub struct SanitizedHtml {
    pub title: Option<String>,
    pub blocks: Vec<BlockCandidate>,
}

#[derive(Clone, Debug)]
pub struct BlockCandidate {
    pub text: String,
    pub dom_path: String,
    pub heading_chain: Vec<String>,
    pub depth: usize,
    pub tag: String,
    pub position: usize,
}

pub fn sanitize_html(input: &str, cfg: &HtmlPreprocessConfig) -> SanitizedHtml {
    let document = Html::parse_document(input);
    let title = extract_title(&document);
    let drop_tags: HashSet<String> = cfg
        .drop_tags
        .iter()
        .map(|tag| tag.to_ascii_lowercase())
        .collect();

    let mut blocks = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();
    let mut path_stack = vec!["html[0]".to_string()];
    let mut position = 0usize;

    let root = document.root_element();
    let mut state = TraverseState {
        drop_tags: &drop_tags,
        cfg,
        heading_stack: &mut heading_stack,
        position: &mut position,
        blocks: &mut blocks,
    };
    traverse_element(root, &mut path_stack, 0, &mut state);

    SanitizedHtml { title, blocks }
}

fn extract_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    let element = document.select(&selector).next()?;
    let text = element.text().collect::<Vec<_>>().join(" ");
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

struct TraverseState<'a> {
    drop_tags: &'a HashSet<String>,
    cfg: &'a HtmlPreprocessConfig,
    heading_stack: &'a mut Vec<(usize, String)>,
    position: &'a mut usize,
    blocks: &'a mut Vec<BlockCandidate>,
}

fn traverse_element(
    element: ElementRef,
    path_stack: &mut Vec<String>,
    depth: usize,
    state: &mut TraverseState<'_>,
) {
    let tag = element.value().name().to_ascii_lowercase();
    if state.drop_tags.contains(&tag) {
        return;
    }

    let dom_path = path_stack.join(" > ");
    let mut heading_chain_snapshot: Vec<String> = state
        .heading_stack
        .iter()
        .map(|(_, text)| text.clone())
        .collect();

    if let Some(level) = heading_level(&tag) {
        let heading_text = collect_text(element, state.drop_tags, state.cfg.preserve_whitespace);
        let trimmed = heading_text.trim();
        if !trimmed.is_empty() {
            update_heading_stack(state.heading_stack, level, trimmed.to_string());
            heading_chain_snapshot = state
                .heading_stack
                .iter()
                .map(|(_, text)| text.clone())
                .collect();
            push_block(
                state.blocks,
                trimmed.to_string(),
                dom_path.clone(),
                heading_chain_snapshot.clone(),
                depth,
                tag.clone(),
                *state.position,
            );
            *state.position += 1;
        }
    } else if should_emit_block(&tag) {
        let mut text = collect_text(element, state.drop_tags, state.cfg.preserve_whitespace);
        text = text.trim().to_string();
        if !text.is_empty() {
            if tag == "li" {
                text = format!("- {}", text);
            }
            push_block(
                state.blocks,
                text,
                dom_path.clone(),
                heading_chain_snapshot.clone(),
                depth,
                tag.clone(),
                *state.position,
            );
            *state.position += 1;
        }
    }

    let mut child_index = 0usize;
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let child_tag = child_element.value().name().to_ascii_lowercase();
            path_stack.push(format!("{}[{}]", child_tag, child_index));
            traverse_element(child_element, path_stack, depth + 1, state);
            path_stack.pop();
            child_index += 1;
        }
    }
}

fn should_emit_block(tag: &str) -> bool {
    BLOCK_TAGS.contains(&tag)
}

fn heading_level(tag: &str) -> Option<usize> {
    HEADING_TAGS
        .iter()
        .find(|(name, _)| name == &tag)
        .map(|(_, level)| *level)
}

fn preserves_whitespace(tag: &str) -> bool {
    PRESERVE_WHITESPACE_TAGS.contains(&tag)
}

fn update_heading_stack(stack: &mut Vec<(usize, String)>, level: usize, text: String) {
    while let Some((existing_level, _)) = stack.last() {
        if *existing_level >= level {
            stack.pop();
        } else {
            break;
        }
    }
    stack.push((level, text));
}

fn push_block(
    blocks: &mut Vec<BlockCandidate>,
    text: String,
    dom_path: String,
    heading_chain: Vec<String>,
    depth: usize,
    tag: String,
    position: usize,
) {
    blocks.push(BlockCandidate {
        text,
        dom_path,
        heading_chain,
        depth,
        tag,
        position,
    });
}

fn collect_text(
    element: ElementRef,
    drop_tags: &HashSet<String>,
    preserve_whitespace_flag: bool,
) -> String {
    let mut buffer = String::new();
    let root_preserve = preserve_whitespace_flag || preserves_whitespace(element.value().name());
    collect_text_element(element, drop_tags, &mut buffer, root_preserve);
    if preserve_whitespace_flag || root_preserve {
        buffer
    } else {
        normalize_whitespace(&buffer)
    }
}

fn collect_text_element(
    element: ElementRef,
    drop_tags: &HashSet<String>,
    buffer: &mut String,
    parent_preserve: bool,
) {
    let tag = element.value().name().to_ascii_lowercase();
    if drop_tags.contains(&tag) {
        return;
    }
    let preserve = parent_preserve || preserves_whitespace(&tag);
    for child in element.children() {
        match child.value() {
            Node::Element(_) => {
                if let Some(child_element) = ElementRef::wrap(child) {
                    collect_text_element(child_element, drop_tags, buffer, preserve);
                }
            }
            Node::Text(text_node) => append_text(text_node, buffer, preserve),
            _ => {}
        }
    }
}

fn append_text(text_node: &scraper::node::Text, buffer: &mut String, preserve: bool) {
    let content = text_node.to_string();
    if preserve {
        buffer.push_str(&content);
    } else {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(trimmed);
        }
    }
}

fn normalize_whitespace(text: &str) -> String {
    let mut normalized = String::new();
    let mut prev_whitespace = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_whitespace {
                normalized.push(' ');
                prev_whitespace = true;
            }
        } else {
            prev_whitespace = false;
            normalized.push(ch);
        }
    }
    normalized.trim().to_string()
}
