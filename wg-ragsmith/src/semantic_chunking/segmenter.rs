use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use regex::Regex;
#[cfg(feature = "semantic-chunking-segtok")]
use segtok::segmenter::{split_single, SegmentConfig};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SentenceSplitter {
    #[default]
    Regex,
    #[cfg(feature = "semantic-chunking-segtok")]
    Segtok,
}

pub fn split_sentences(text: &str, mode: SentenceSplitter) -> Vec<String> {
    match mode {
        SentenceSplitter::Regex => regex_split(text),
        #[cfg(feature = "semantic-chunking-segtok")]
        SentenceSplitter::Segtok => {
            let cfg = SegmentConfig::default();
            let sentences = split_single(text, cfg);
            let collected: Vec<String> = sentences
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if collected.is_empty() {
                regex_split(text)
            } else {
                collected
            }
        }
    }
}

fn regex_split(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let re = SENTENCE_SPLIT_REGEX.get_or_init(|| {
        Regex::new(r"(?ms)(.*?[:.!?](?:\s+|$)|.+$)").expect("valid sentence regex")
    });
    let mut segments = Vec::new();
    for capture in re.captures_iter(text) {
        if let Some(mat) = capture.get(0) {
            let sentence = mat.as_str().trim();
            if !sentence.is_empty() {
                segments.push(sentence.to_string());
            }
        }
    }
    if segments.is_empty() {
        segments.push(text.trim().to_string());
    }
    segments
}

static SENTENCE_SPLIT_REGEX: OnceLock<Regex> = OnceLock::new();
