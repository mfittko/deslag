//! Scorer, non-inferiority bound and shuffle checks against hand-computed values.

use deslag_eval::score::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn normalizes_in_declared_order() {
    assert_eq!(normalize("  a \n\t b  "), "a b");
    assert_eq!(normalize("\"ledger-sync\""), "ledger-sync");
    assert_eq!(normalize("`x`"), "x");
    assert_eq!(normalize("'x'"), "x");
    assert_eq!(normalize("\"x"), "\"x");
    assert_eq!(normalize("\"\"x\"\""), "\"x\"");
    assert_eq!(normalize("1873."), "1873");
    assert_eq!(normalize("a.."), "a.");
    assert_eq!(normalize("\""), "\"");
}

#[test]
fn scores_each_rule() {
    let acc = s(&["Priya Raman"]);
    assert_eq!(
        score("Priya Raman.", &acc, Rule::Exact, 0.0),
        ErrorKind::None
    );
    assert_eq!(
        score("priya raman", &acc, Rule::Exact, 0.0),
        ErrorKind::Wrong
    );
    assert_eq!(
        score("priya raman", &acc, Rule::ExactCi, 0.0),
        ErrorKind::None
    );
    let set = s(&["5127,5133,5190"]);
    assert_eq!(
        score("5190, 5127 ,5133", &set, Rule::Set, 0.0),
        ErrorKind::None
    );
    assert_eq!(score("5190,5127", &set, Rule::Set, 0.0), ErrorKind::Wrong);
    assert_eq!(
        score("5190,5127,5133,5000", &set, Rule::Set, 0.0),
        ErrorKind::Wrong
    );
    let num = s(&["1873"]);
    assert_eq!(score("1873.0", &num, Rule::Number, 0.0), ErrorKind::None);
    assert_eq!(score("1874", &num, Rule::Number, 0.0), ErrorKind::Wrong);
    assert_eq!(score("1874", &num, Rule::Number, 1.0), ErrorKind::None);
    assert_eq!(score("NaN", &num, Rule::Number, 1.0), ErrorKind::Wrong);
    assert_eq!(
        score("about 1873", &num, Rule::Number, 0.0),
        ErrorKind::Wrong
    );
    assert_eq!(
        score("2", &s(&["1", "2"]), Rule::Number, 0.0),
        ErrorKind::None
    );
}

#[test]
fn abstentions_are_separate_from_wrong_answers() {
    let acc = s(&["x"]);
    assert_eq!(score("", &acc, Rule::Exact, 0.0), ErrorKind::Abstain);
    assert_eq!(score("  ", &acc, Rule::Exact, 0.0), ErrorKind::Abstain);
    assert_eq!(
        score("not_found", &acc, Rule::Exact, 0.0),
        ErrorKind::Abstain
    );
    assert_eq!(
        score("`NOT_FOUND`", &acc, Rule::Exact, 0.0),
        ErrorKind::Abstain
    );
    assert_eq!(score("y", &acc, Rule::Exact, 0.0), ErrorKind::Wrong);
}

#[test]
fn agresti_min_bound_matches_hand_values() {
    // All concordant: lower = -1.6449 / (n + 2).
    assert!((lower_bound(100, 0, 0) - (-0.016126)).abs() < 1e-5);
    assert!((lower_bound(50, 0, 0) - (-0.031633)).abs() < 1e-5);
    // n = 100, b = 3, c = 1: d' = -2/102, var = (5 - 4/102) / 102^2.
    let want = -2.0 / 102.0 - 1.6449 * (5.0_f64 - 4.0 / 102.0).sqrt() / 102.0;
    assert!((lower_bound(100, 3, 1) - want).abs() < 1e-12);
    assert!((want - (-0.055526)).abs() < 1e-5);
}

#[test]
fn minimum_pilot_size_is_81() {
    assert_eq!(verdict(80, 0, 0), "not shown");
    assert_eq!(verdict(81, 0, 0), "passes");
    assert_eq!(verdict(10, 3, 0), "inferior");
    assert_eq!(verdict(0, 0, 0), "no data");
}

#[test]
fn shuffle_is_a_seeded_permutation() {
    let base: Vec<u32> = (0..61).collect();
    let (mut a, mut b, mut c) = (base.clone(), base.clone(), base.clone());
    shuffle(&mut a, 16);
    shuffle(&mut b, 16);
    shuffle(&mut c, 17);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, base);
    a.sort();
    assert_eq!(a, base);
}
