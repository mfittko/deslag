//! Answer scoring, the paired non-inferiority bound and the seeded run order.
//! `docs/benchmark-methodology.md` predeclares every rule in this module.

use std::collections::BTreeSet;

/// Non-inferiority margin: -2 percentage points.
pub const MARGIN: f64 = 0.02;
/// One-sided 95% normal quantile.
const Z: f64 = 1.6449;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    Exact,
    ExactCi,
    Set,
    Number,
}

impl Rule {
    pub fn parse(s: &str) -> Option<Rule> {
        match s {
            "exact" => Some(Rule::Exact),
            "exact_ci" => Some(Rule::ExactCi),
            "set" => Some(Rule::Set),
            "number" => Some(Rule::Number),
            _ => None,
        }
    }
}

/// Outcome of one run. Every kind except `None` is a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    None,
    /// A non-empty answer that scores wrong: an exact-value or evidence error.
    Wrong,
    /// Empty or `NOT_FOUND`.
    Abstain,
    /// The backend call failed or reported an error.
    Backend,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::None => "none",
            ErrorKind::Wrong => "wrong",
            ErrorKind::Abstain => "abstain",
            ErrorKind::Backend => "backend",
        }
    }
}

/// Trim, collapse whitespace runs, strip one pair of surrounding quotes or
/// backticks, strip one trailing period. In that order.
pub fn normalize(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut t = collapsed.as_str();
    for q in ['"', '\'', '`'] {
        if t.len() >= 2 && t.starts_with(q) && t.ends_with(q) {
            t = &t[1..t.len() - 1];
            break;
        }
    }
    t.strip_suffix('.').unwrap_or(t).to_string()
}

fn items(s: &str) -> BTreeSet<String> {
    s.split(',')
        .map(normalize)
        .filter(|i| !i.is_empty())
        .collect()
}

pub fn score(answer: &str, accepted: &[String], rule: Rule, tolerance: f64) -> ErrorKind {
    let a = normalize(answer);
    if a.is_empty() || a.eq_ignore_ascii_case("NOT_FOUND") {
        return ErrorKind::Abstain;
    }
    let hit = accepted.iter().any(|x| {
        let x = normalize(x);
        match rule {
            Rule::Exact => a == x,
            Rule::ExactCi => a.to_lowercase() == x.to_lowercase(),
            // The whole-string path keeps a fully quoted list. The raw path
            // strips quotes from each item, edge items included.
            Rule::Set => items(&a) == items(&x) || items(answer) == items(&x),
            Rule::Number => match (a.parse::<f64>(), x.parse::<f64>()) {
                // Epsilon absorbs binary rounding at a decimal tolerance boundary.
                // A few ulps, so tolerance 0 never widens into a whole unit.
                // A non-finite value never matches: an infinite epsilon would accept anything.
                (Ok(p), Ok(q)) if p.is_finite() && q.is_finite() => {
                    (p - q).abs() <= tolerance + 4.0 * f64::EPSILON * p.abs().max(q.abs()).max(1.0)
                }
                _ => false,
            },
        }
    });
    if hit {
        ErrorKind::None
    } else {
        ErrorKind::Wrong
    }
}

/// Agresti-Min adjusted difference (arm minus baseline) and its standard error
/// over `n` pairs. `b` counts pairs where only the baseline succeeded, `c`
/// pairs where only the arm succeeded. Panics when `b + c > n`, an impossible
/// paired table.
fn adjusted(n: u32, b: u32, c: u32) -> (f64, f64) {
    assert!(
        u64::from(b) + u64::from(c) <= u64::from(n),
        "discordant pairs b + c exceed n"
    );
    let n2 = f64::from(n) + 2.0;
    let (b, c) = (f64::from(b) + 0.5, f64::from(c) + 0.5);
    let var = ((b + c) - (c - b).powi(2) / n2) / (n2 * n2);
    ((c - b) / n2, var.sqrt())
}

/// Lower one-sided 95% bound on arm-minus-baseline success, Agresti-Min
/// adjusted Wald for matched proportions.
pub fn lower_bound(n: u32, b: u32, c: u32) -> f64 {
    let (d, se) = adjusted(n, b, c);
    d - Z * se
}

/// Upper one-sided 95% bound, the mirror of [`lower_bound`].
pub fn upper_bound(n: u32, b: u32, c: u32) -> f64 {
    let (d, se) = adjusted(n, b, c);
    d + Z * se
}

/// `passes`, `inferior`, `not shown`, or `no data` when `n == 0`. `inferior`
/// means the upper bound is below the margin, so inferiority is shown at the
/// same level as a pass. Panics when `b + c > n`.
pub fn verdict(n: u32, b: u32, c: u32) -> &'static str {
    if n == 0 {
        assert!(b == 0 && c == 0, "discordant pairs b + c exceed n");
        "no data"
    } else if lower_bound(n, b, c) > -MARGIN {
        "passes"
    } else if upper_bound(n, b, c) < -MARGIN {
        "inferior"
    } else {
        "not shown"
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Fisher-Yates driven by splitmix64. The same seed yields the same order.
// ponytail: modulo draw, its bias is below 1e-15 for any corpus that fits in memory.
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        let j = (splitmix64(&mut state) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn splitmix64_matches_reference() {
        let mut state = 0;
        assert_eq!(super::splitmix64(&mut state), 0xE220_A839_7B1D_CDAF);
    }
}
