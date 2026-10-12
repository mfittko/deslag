//! Reference JSON minifier shared by the fuzz targets.

/// Drops JSON whitespace outside strings. Valid input only; strings are copied verbatim.
pub fn reference_minify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let (mut in_string, mut escaped) = (false, false);
    for c in input.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if matches!(c, ' ' | '\t' | '\n' | '\r') {
            continue;
        } else {
            in_string = c == '"';
            out.push(c);
        }
    }
    out
}
