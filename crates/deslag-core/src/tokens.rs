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
/// - ASCII punctuation: one token per started 3 characters.
/// - Other characters: one token each.
fn estimate_tokens(text: &str) -> u64 {
    let mut total = 0u64;
    let mut chars = text.chars().peekable();
    while let Some(first) = chars.next() {
        let kind = class(first);
        let mut len = 1u64;
        let mut single_space = first == ' ';
        while chars.peek().is_some_and(|&c| class(c) == kind) {
            single_space = false;
            chars.next();
            len += 1;
        }
        total += match kind {
            Class::Letter => len.div_ceil(7),
            Class::Digit => len.div_ceil(3),
            Class::Space if single_space && chars.peek().is_some() => 0,
            Class::Space => 1,
            Class::Punct => len.div_ceil(3),
            Class::Other => len,
        };
    }
    total
}
