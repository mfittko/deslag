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
    // Quotes are stripped before the trailing period.
    assert_eq!(normalize("\"x\"."), "\"x\"");
    assert_eq!(normalize("\"x.\""), "x");
}

#[test]
fn parses_manifest_rule_names() {
    assert_eq!(Rule::parse("exact"), Some(Rule::Exact));
    assert_eq!(Rule::parse("exact_ci"), Some(Rule::ExactCi));
    assert_eq!(Rule::parse("set"), Some(Rule::Set));
    assert_eq!(Rule::parse("number"), Some(Rule::Number));
    assert_eq!(Rule::parse("EXACT"), None);
    assert_eq!(Rule::parse(""), None);
}

#[test]
fn set_rule_drops_empty_items_and_is_case_sensitive() {
    let set = s(&["5127,5133,5190"]);
    assert_eq!(
        score("5127,,5133,5190,", &set, Rule::Set, 0.0),
        ErrorKind::None
    );
    assert_eq!(score("a,b", &s(&["a,B"]), Rule::Set, 0.0), ErrorKind::Wrong);
    let set_score = |a: &str| score(a, &set, Rule::Set, 0.0);
    assert_eq!(set_score("\"5127\", \"5133\", \"5190\""), ErrorKind::None);
    assert_eq!(set_score("`5127`, `5133`, `5190`"), ErrorKind::None);
    assert_eq!(set_score("\"5127,5133,5190\""), ErrorKind::None);
    assert_eq!(set_score("5190, 5127 ,5133"), ErrorKind::None);
    assert_eq!(set_score("\"5127\", \"5133\""), ErrorKind::Wrong);
    assert_eq!(
        score("\"a\", \"b\"", &s(&["a,B"]), Rule::Set, 0.0),
        ErrorKind::Wrong
    );
}

#[test]
fn number_tolerance_holds_at_decimal_boundaries() {
    let n = |a: &str, x: &str, t: f64| score(a, &s(&[x]), Rule::Number, t);
    assert_eq!(n("1.1", "1.0", 0.1), ErrorKind::None);
    assert_eq!(n("0.3", "0.2", 0.1), ErrorKind::None);
    assert_eq!(n("5", "5", 0.0), ErrorKind::None);
    assert_eq!(n("1.2", "1.0", 0.1), ErrorKind::Wrong);
    assert_eq!(n("6", "5", 0.0), ErrorKind::Wrong);
    assert_eq!(n("1000000000", "1000000000", 0.0), ErrorKind::None);
    assert_eq!(n("18342", "18342", 0.0), ErrorKind::None);
    assert_eq!(n("0.30000000000000004", "0.3", 0.0), ErrorKind::None);
    assert_eq!(n("1000000001", "1000000000", 0.0), ErrorKind::Wrong);
    assert_eq!(n("18343", "18342", 0.0), ErrorKind::Wrong);
    for bad in ["inf", "-inf", "infinity", "1e309", "NaN"] {
        assert_eq!(n(bad, "5", 0.0), ErrorKind::Wrong, "{bad}");
    }
}

#[test]
fn accepts_possible_paired_tables() {
    assert!(lower_bound(10, 2, 3).is_finite());
    assert!(upper_bound(5, 5, 0).is_finite());
    assert_eq!(verdict(0, 0, 0), "no data");
}

#[test]
#[should_panic(expected = "exceed n")]
fn rejects_more_discordant_pairs_than_tasks() {
    verdict(1, 5, 0);
}

#[test]
#[should_panic(expected = "exceed n")]
fn rejects_discordant_pairs_summing_past_n() {
    verdict(3, 2, 2);
}

#[test]
#[should_panic(expected = "exceed n")]
fn rejects_discordant_pairs_with_no_tasks() {
    verdict(0, 1, 0);
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
    // n = 20, b = 8, c = 0: d' = -8/22, var = (9 - 64/22) / 22^2.
    let want = -8.0 / 22.0 + 1.6449 * (9.0_f64 - 64.0 / 22.0).sqrt() / 22.0;
    assert!((upper_bound(20, 8, 0) - want).abs() < 1e-12);
    assert!((want - (-0.179109)).abs() < 1e-5);
}

#[test]
fn minimum_pilot_size_is_81() {
    assert_eq!(verdict(80, 0, 0), "not shown");
    assert_eq!(verdict(81, 0, 0), "passes");
    assert_eq!(verdict(0, 0, 0), "no data");
}

#[test]
fn inferior_needs_the_upper_bound_below_the_margin() {
    // Point estimate -30 pp, but the upper bound is about -0.3 pp.
    assert!(upper_bound(10, 3, 0) > -0.02);
    assert_eq!(verdict(10, 3, 0), "not shown");
    assert_eq!(verdict(20, 8, 0), "inferior");
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

#[test]
fn shuffle_order_is_pinned_for_a_recorded_seed() {
    let mut v: Vec<u32> = (0..10).collect();
    shuffle(&mut v, 16);
    assert_eq!(v, [2, 1, 0, 3, 9, 6, 7, 8, 4, 5]);
}
