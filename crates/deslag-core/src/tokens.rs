//! Local token estimation. No network, no tokenizer tables.

use crate::{TokenCount, TokenCounter, TokenMethod};

/// Deterministic character-class heuristic. Every count it returns is labeled
/// [`TokenMethod::Estimate`] with [`EstimateCounter::TOKENIZER`].
///
/// Calibration against offline references and the measured error bounds live
/// in `tests/fixtures/tokens/README.md`. Changing the heuristic changes counts,
/// so it must bump the version in the label.
// ponytail: fixed class weights, swap in a real per-model tokenizer when one is
// verified for the target family.
#[derive(Clone, Copy, Debug, Default)]
pub struct EstimateCounter;

impl EstimateCounter {
    pub const TOKENIZER: &'static str = "deslag-estimate-v1";
}

impl TokenCounter for EstimateCounter {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: estimate_tokens(text),
            method: TokenMethod::Estimate,
            tokenizer: Self::TOKENIZER,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Letter,
    Digit,
    Space,
    Punct,
    Other,
}

fn class(c: char) -> Class {
    match c {
        'a'..='z' | 'A'..='Z' => Class::Letter,
        '0'..='9' => Class::Digit,
        c if c.is_whitespace() => Class::Space,
        c if c.is_ascii() => Class::Punct,
        _ => Class::Other,
    }
}

/// Sums a cost per run of same-class characters:
/// - ASCII letters: one token per started 7 characters.
/// - ASCII digits: one token per started 3 digits.
/// - Whitespace: a single space before non-whitespace joins the next token
///   and costs nothing; any other run costs one token.
/// - ASCII punctuation: a run of up to 3 characters costs one token; a
///   longer run costs one token per started 2 characters, because tokenizers
///   split dense bracket runs such as `":{},"`.
/// - Other characters: one token each, two for a non-BMP character.
fn estimate_tokens(text: &str) -> u64 {
    let mut total = 0u64;
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.next() {
        let kind = class(first);
        let mut len = 1u64;
        let mut other = utf8_cost(first);
        let mut single_space = first == ' ';
        while let Some(c) = chars.next_if(|&c| class(c) == kind) {
            single_space = false;
            len += 1;
            other += utf8_cost(c);
        }
        total += match kind {
            Class::Letter => len.div_ceil(7),
            Class::Digit => len.div_ceil(3),
            Class::Space if single_space && chars.peek().is_some() => 0,
            Class::Space => 1,
            Class::Punct if len <= 3 => 1,
            Class::Punct => len.div_ceil(2),
            Class::Other => other,
        };
    }
    total
}

/// One token per character up to three UTF-8 bytes, two for a non-BMP
/// (four-byte) character such as an emoji.
fn utf8_cost(c: char) -> u64 {
    c.len_utf8().div_ceil(3) as u64
}
