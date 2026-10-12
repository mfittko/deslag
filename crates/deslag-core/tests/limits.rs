use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use deslag_core::{
    BoundedExecutor, CompressionContext, FIRST_SLICE_JSON_MINIFY_CONTEXT, Limits,
    MAX_JSON_DEPTH_CEILING, Mode, Outcome, Reason, Rejected, compress,
};

fn ctx(json_minify: bool, json_table: bool, exact_line_runs: bool) -> CompressionContext<'static> {
    CompressionContext {
        mode: Mode::Safe,
        json_minify,
        json_table,
        exact_line_runs,
        limits: Limits {
            min_candidate_bytes: 0,
            max_candidate_bytes: 64,
            ..Limits::default()
        },
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

fn assert_skipped(input: &str, ctx: &CompressionContext, reason: Reason) {
    let result = compress(input, ctx);
    assert_eq!(result.report.outcome, Outcome::Skipped);
    assert_eq!(result.report.reason, Some(reason));
    assert!(matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input)));
}

#[test]
fn oversize_input_skips_for_every_transform() {
    let json = format!("[{}1]", "1, ".repeat(40));
    let text = "line\n".repeat(20);
    assert_skipped(&json, &ctx(true, false, false), Reason::AboveMaxBytes);
    assert_skipped(&json, &ctx(false, true, false), Reason::AboveMaxBytes);
    assert_skipped(&text, &ctx(false, false, true), Reason::AboveMaxBytes);
}

fn nested(depth: usize) -> String {
    format!("{}1{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn depth_beyond_limit_skips_minify_and_table() {
    let mut c = ctx(true, false, false);
    c.limits.max_candidate_bytes = usize::MAX;
    c.limits.max_json_depth = 4;
    assert_skipped(&nested(5), &c, Reason::DepthLimit);
    let mut c = ctx(false, true, false);
    c.limits.max_candidate_bytes = usize::MAX;
    c.limits.max_json_depth = 4;
    assert_skipped(&nested(5), &c, Reason::DepthLimit);
}

#[test]
fn absurd_depth_limit_is_clamped_and_does_not_overflow_the_stack() {
    let input = nested(MAX_JSON_DEPTH_CEILING + 1);
    for (minify, table) in [(true, false), (false, true)] {
        let mut c = ctx(minify, table, false);
        c.limits.max_candidate_bytes = usize::MAX;
        c.limits.max_json_depth = usize::MAX;
        assert_skipped(&input, &c, Reason::DepthLimit);
    }
}

#[test]
fn table_row_cap_is_enforced() {
    let rows = |n: usize| {
        let items: Vec<String> = (0..n)
            .map(|i| format!("  {{ \"id\": {i}, \"name\": \"user-{i}\", \"active\": true }}"))
            .collect();
        format!("[\n{}\n]", items.join(",\n"))
    };
    let mut c = ctx(false, true, false);
    c.limits.max_candidate_bytes = usize::MAX;
    c.limits.max_table_rows = 50;
    assert_eq!(compress(&rows(50), &c).report.outcome, Outcome::Changed);
    let over = rows(51);
    let result = compress(&over, &c);
    assert_eq!(result.report.outcome, Outcome::Skipped);
    assert_eq!(result.report.reason, Some(Reason::UnsupportedFormat));
    assert_ne!(result.report.outcome, Outcome::Changed);
    assert!(matches!(&result.output, Cow::Borrowed(_)));
}

/// Spins until all permits are released, failing after 5 s.
fn wait_idle(exec: &BoundedExecutor) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while exec.in_flight() != 0 {
        assert!(Instant::now() < deadline, "permit not released within 5 s");
        std::thread::yield_now();
    }
}

const BUDGET: Duration = Duration::from_millis(10);

#[test]
fn job_inside_budget_returns_value() {
    let exec = BoundedExecutor::new(2, Duration::from_secs(5)).unwrap();
    assert_eq!(exec.run(|| 7), Ok(7));
    assert_eq!(exec.in_flight(), 0);
}

#[test]
fn expired_job_holds_its_permit_until_it_ends() {
    let exec = BoundedExecutor::new(1, BUDGET).unwrap();
    let (release, gate) = mpsc::channel::<()>();
    let (done, ended) = mpsc::channel::<()>();
    let result = exec.run(move || {
        let _ = gate.recv();
        let _ = done.send(());
    });
    assert_eq!(result, Err(Rejected::BudgetExceeded));
    assert_eq!(exec.in_flight(), 1);
    assert_eq!(exec.run(|| ()), Err(Rejected::Busy));
    release.send(()).unwrap();
    ended.recv().unwrap();
    // The permit is released right after the job returns; wait for it without a fixed sleep.
    wait_idle(&exec);
    assert_eq!(exec.run(|| 1), Ok(1));
}

/// Signals when its destructor starts, then blocks until released.
struct SlowDrop {
    dropping: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl Drop for SlowDrop {
    fn drop(&mut self) {
        let _ = self.dropping.send(());
        let _ = self.release.recv();
    }
}

#[test]
fn expired_job_holds_its_permit_while_its_value_drops() {
    let exec = BoundedExecutor::new(1, BUDGET).unwrap();
    let (start, gate) = mpsc::channel::<()>();
    let (dropping, drop_started) = mpsc::channel::<()>();
    let (release_drop, release) = mpsc::channel::<()>();
    let result = exec.run(move || {
        let _ = gate.recv();
        SlowDrop { dropping, release }
    });
    assert!(matches!(result, Err(Rejected::BudgetExceeded)));
    start.send(()).unwrap();
    drop_started.recv().unwrap();
    assert_eq!(exec.in_flight(), 1);
    assert!(matches!(exec.run(|| ()), Err(Rejected::Busy)));
    release_drop.send(()).unwrap();
    wait_idle(&exec);
    assert_eq!(exec.run(|| 1), Ok(1));
}

#[test]
fn busy_executor_spawns_nothing() {
    let exec = BoundedExecutor::new(1, BUDGET).unwrap();
    let (release, gate) = mpsc::channel::<()>();
    assert_eq!(
        exec.run(move || {
            let _ = gate.recv();
        }),
        Err(Rejected::BudgetExceeded)
    );
    let ran = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&ran);
    let result = exec.run(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(result, Err(Rejected::Busy));
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    release.send(()).unwrap();
}

#[test]
fn panicking_job_is_rejected_and_releases_its_permit() {
    let exec = BoundedExecutor::new(1, Duration::from_secs(5)).unwrap();
    let result: Result<(), Rejected> = exec.run(|| panic!("job failure"));
    assert_eq!(result, Err(Rejected::Panicked));
    wait_idle(&exec);
    assert_eq!(exec.run(|| 1), Ok(1));
}

#[test]
fn invalid_construction_is_rejected() {
    assert!(BoundedExecutor::new(0, BUDGET).is_none());
    assert!(BoundedExecutor::new(1, Duration::ZERO).is_none());
    let default = BoundedExecutor::default();
    assert_eq!(default.max_parallel_jobs(), 4);
    assert_eq!(default.processing_budget(), BUDGET);
}

#[test]
fn rejected_reasons_are_stable() {
    assert_eq!(Rejected::Busy.as_str(), "busy");
    assert_eq!(Rejected::BudgetExceeded.as_str(), "budget_exceeded");
    assert_eq!(Rejected::Panicked.as_str(), "job_panicked");
}
