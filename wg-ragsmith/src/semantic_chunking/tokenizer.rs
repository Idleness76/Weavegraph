use std::sync::OnceLock;

#[cfg(feature = "semantic-chunking-tiktoken")]
use tiktoken_rs::CoreBPE;

/// Estimate token counts for a single string, leveraging tiktoken when available.
pub fn count(text: &str) -> usize {
    #[cfg(feature = "semantic-chunking-tiktoken")]
    {
        if let Some(encoder) = encoder() {
            return encoder.encode_with_special_tokens(text).len();
        }
    }

    fallback_count(text)
}

/// Compute token counts for a batch of strings.
pub fn batch_count<'a, I>(texts: I) -> Vec<usize>
where
    I: IntoIterator<Item = &'a str>,
{
    #[cfg(feature = "semantic-chunking-tiktoken")]
    {
        if let Some(encoder) = encoder() {
            return texts
                .into_iter()
                .map(|text| encoder.encode_with_special_tokens(text).len())
                .collect();
        }
    }

    texts.into_iter().map(fallback_count).collect()
}

fn fallback_count(text: &str) -> usize {
    let count = text.split_whitespace().count();
    if count == 0 && !text.is_empty() {
        1
    } else {
        count
    }
}

#[cfg(feature = "semantic-chunking-tiktoken")]
fn encoder() -> Option<&'static CoreBPE> {
    static ENCODER: OnceLock<Option<CoreBPE>> = OnceLock::new();
    ENCODER
        .get_or_init(|| tiktoken_rs::cl100k_base().ok())
        .as_ref()
}
