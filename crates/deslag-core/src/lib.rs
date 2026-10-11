//! Shared core for deslag.

use std::collections::HashSet;

/// Crate version, as set in the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable transform identity for the JSON minifier.
pub const JSON_MINIFY_ID: &str = "json-minify";
/// Bumped whenever `minify_json` output can change for some input.
pub const JSON_MINIFY_VERSION: u32 = 1;

/// Containers nested deeper than this are skipped, which bounds recursion.
const MAX_DEPTH: usize = 128;

/// Removes insignificant whitespace from a JSON object or array.
///
/// Every string, number and literal lexeme is copied byte for byte, so values,
/// escapes and number precision are preserved. Member and element order is kept.
///
/// `None` means: pass the input through unchanged. It is returned when:
/// - `input` is a bare scalar, invalid or truncated JSON, or has trailing data;
/// - any object has a duplicate key, because consumers disagree on which wins;
/// - any object key contains an escape, because it could hide a duplicate;
/// - containers nest deeper than 128 levels;
/// - the input is already minimal, so the result would not be shorter.
pub fn minify_json(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut s = Scanner {
        b: bytes,
        i: 0,
        out: Vec::with_capacity(bytes.len()),
    };
    s.ws();
    // Bare scalars are treated as plain text.
    if !matches!(s.peek(), Some(b'{' | b'[')) {
        return None;
    }
    s.value(0)?;
    s.ws();
    if s.i != bytes.len() || s.out.len() >= bytes.len() {
        return None;
    }
    // Output is ASCII structure plus whole string lexemes from valid UTF-8 input.
    String::from_utf8(s.out).ok()
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    out: Vec<u8>,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    /// JSON whitespace only; any other byte outside a lexeme is invalid.
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.out.push(c);
            self.i += 1;
        }
        hit
    }

    fn expect(&mut self, c: u8) -> Option<()> {
        self.eat(c).then_some(())
    }

    fn value(&mut self, depth: usize) -> Option<()> {
        match self.peek()? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => self.string().map(drop),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => self.number(),
        }
    }

    fn object(&mut self, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.expect(b'{')?;
        self.ws();
        if self.eat(b'}') {
            return Some(());
        }
        let mut keys = HashSet::new();
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = self.string()?;
            // ponytail: keys are compared as raw lexemes, and any escaped key is
            // skipped. An escaped key such as `"\u0061"` decodes to the same key as
            // `"a"`, so the two could hide a duplicate. Decode keys
            // if escaped keys turn out to be common in real tool output.
            if key.contains(&b'\\') || !keys.insert(key) {
                return None;
            }
            self.ws();
            self.expect(b':')?;
            self.ws();
            self.value(depth)?;
            self.ws();
            if !self.eat(b',') {
                return self.expect(b'}');
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.expect(b'[')?;
        self.ws();
        if self.eat(b']') {
            return Some(());
        }
        loop {
            self.ws();
            self.value(depth)?;
            self.ws();
            if !self.eat(b',') {
                return self.expect(b']');
            }
        }
    }

    /// Validates one string lexeme, copies it verbatim and returns it with its quotes.
    fn string(&mut self) -> Option<&'a [u8]> {
        let start = self.i;
        self.i += 1;
        loop {
            match self.peek()? {
                b'"' => break,
                b'\\' => {
                    self.i += 1;
                    match self.peek()? {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.i += 1,
                        b'u' => {
                            let hex = self.b.get(self.i + 1..self.i + 5)?;
                            if !hex.iter().all(u8::is_ascii_hexdigit) {
                                return None;
                            }
                            self.i += 5;
                        }
                        _ => return None,
                    }
                }
                c if c < 0x20 => return None,
                _ => self.i += 1,
            }
        }
        self.i += 1;
        let lexeme = &self.b[start..self.i];
        self.out.extend_from_slice(lexeme);
        Some(lexeme)
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        if !self.b[self.i..].starts_with(word) {
            return None;
        }
        self.out.extend_from_slice(word);
        self.i += word.len();
        Some(())
    }

    /// RFC 8259 number grammar, copied verbatim.
    fn number(&mut self) -> Option<()> {
        let start = self.i;
        self.skip(b'-');
        match self.peek()? {
            b'0' => self.i += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        if self.skip(b'.') {
            self.require_digits()?;
        }
        if self.skip(b'e') || self.skip(b'E') {
            let _ = self.skip(b'+') || self.skip(b'-');
            self.require_digits()?;
        }
        self.out.extend_from_slice(&self.b[start..self.i]);
        Some(())
    }

    fn skip(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        self.i += usize::from(hit);
        hit
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
    }

    fn require_digits(&mut self) -> Option<()> {
        let start = self.i;
        self.digits();
        (self.i > start).then_some(())
    }
}

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }
}
