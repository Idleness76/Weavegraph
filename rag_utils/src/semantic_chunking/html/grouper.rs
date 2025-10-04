use super::preprocess::SanitizedHtml;

#[derive(Clone, Debug)]
pub struct HtmlBlock {
    pub text: String,
    pub dom_paths: Vec<String>,
    pub heading_chain: Vec<String>,
    pub depth: usize,
    pub tags: Vec<String>,
    pub position_range: (usize, usize),
    pub is_heading: bool,
}

pub fn group_blocks(doc: SanitizedHtml) -> Vec<HtmlBlock> {
    let mut results = Vec::new();
    let mut buffer: Option<HtmlBlock> = None;

    for candidate in doc.blocks {
        if candidate.text.trim().is_empty() {
            continue;
        }
        let is_heading = candidate.tag.starts_with('h');

        if is_heading {
            flush_buffer(&mut buffer, &mut results);
            results.push(HtmlBlock {
                text: candidate.text,
                dom_paths: vec![candidate.dom_path],
                heading_chain: candidate.heading_chain,
                depth: candidate.depth,
                tags: vec![candidate.tag],
                position_range: (candidate.position, candidate.position),
                is_heading: true,
            });
            continue;
        }

        match buffer.as_mut() {
            Some(existing)
                if existing.heading_chain == candidate.heading_chain
                    && !candidate.heading_chain.is_empty() =>
            {
                existing.text.push_str("\n\n");
                existing.text.push_str(candidate.text.trim());
                existing.dom_paths.push(candidate.dom_path);
                existing.tags.push(candidate.tag);
                existing.position_range.1 = candidate.position;
            }
            _ => {
                flush_buffer(&mut buffer, &mut results);
                buffer = Some(HtmlBlock {
                    text: candidate.text,
                    dom_paths: vec![candidate.dom_path],
                    heading_chain: candidate.heading_chain,
                    depth: candidate.depth,
                    tags: vec![candidate.tag],
                    position_range: (candidate.position, candidate.position),
                    is_heading: false,
                });
            }
        }
    }

    flush_buffer(&mut buffer, &mut results);
    results
}

fn flush_buffer(buffer: &mut Option<HtmlBlock>, results: &mut Vec<HtmlBlock>) {
    if let Some(block) = buffer.take() {
        results.push(block);
    }
}
