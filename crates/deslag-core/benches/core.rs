//! Per-stage latency, throughput, allocation and concurrency baseline (PLAN §13.1, §13.3).
//! No thresholds: results are a measured baseline only.
//!
//! To benchmark a new transform, add one entry to `STAGES`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use deslag_core::{
    BoundedExecutor, CompressionContext, EstimateCounter, FIRST_SLICE_JSON_MINIFY_CONTEXT, Mode,
    Rejected, TokenCounter, compress,
};

const SIZES_KIB: [usize; 5] = [2, 16, 64, 256, 1024];
const WARMUP: usize = 10;
const CORPUS_VERSION: &str = "generated-v1";
const CONCURRENCY: [usize; 3] = [1, 8, 32];
const CALLS_PER_THREAD: usize = 200;
const CONCURRENCY_INPUT_KIB: usize = 64;

/// Counts allocations over `System`. Single counters: stage rows run on one thread.
struct Counting;
static COUNT: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(by: usize) {
    COUNT.fetch_add(1, Relaxed);
    BYTES.fetch_add(by, Relaxed);
    let now = CURRENT.fetch_add(by, Relaxed) + by;
    PEAK.fetch_max(now, Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A growing realloc counts as one allocation of the extra bytes.
        if new_size >= layout.size() {
            grow(new_size - layout.size());
        } else {
            CURRENT.fetch_sub(layout.size() - new_size, Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[derive(Clone, Copy)]
enum Kind {
    Repetitive,
    Incompressible,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Repetitive => "repetitive",
            Kind::Incompressible => "incompressible",
        }
    }
}

fn xorshift(x: &mut u64) -> u64 {
    *x ^= *x << 13;
    *x ^= *x >> 7;
    *x ^= *x << 17;
    *x
}

fn random_text(x: &mut u64, len: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .:/-_";
    (0..len)
        .map(|_| char::from(ALPHABET[(xorshift(x) % ALPHABET.len() as u64) as usize]))
        .collect()
}

/// Pretty-printed records with the same keys: minifies, table-eligible.
fn json_repetitive(size: usize) -> String {
    let mut s = String::from("[\n");
    let mut i = 0;
    while s.len() < size {
        let sep = if s.len() == 2 { "" } else { ",\n" };
        s.push_str(&format!(
            "{sep}  {{ \"id\": {i}, \"name\": \"user-{i}\", \"active\": true }}"
        ));
        i += 1;
    }
    s.push_str("\n]\n");
    s
}

/// Already-minified array of pseudo-random strings: a full scan that does not change.
fn json_incompressible(size: usize) -> String {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut s = String::from("[");
    while s.len() < size {
        s.push('"');
        s.push_str(&random_text(&mut x, 32));
        s.push_str("\",");
    }
    s.push_str("null]");
    s
}

/// Table-eligible rows whose values are random text: little gain over minified JSON.
fn table_incompressible(size: usize) -> String {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut s = String::from("[\n");
    let mut i = 0;
    while s.len() < size {
        let sep = if s.len() == 2 { "" } else { ",\n" };
        let (a, b) = (random_text(&mut x, 24), random_text(&mut x, 24));
        s.push_str(&format!(
            "{sep}  {{ \"a\": \"{a}\", \"b\": \"{b}\", \"n\": {i} }}"
        ));
        i += 1;
    }
    s.push_str("\n]\n");
    s
}

/// Log-like text with long runs of identical lines.
fn lines_repetitive(size: usize) -> String {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < size {
        for _ in 0..16 {
            s.push_str(&format!("INFO worker {} heartbeat ok\n", i % 7));
        }
        i += 1;
    }
    s
}

/// Log-like text where no two consecutive lines match.
fn lines_incompressible(size: usize) -> String {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut s = String::new();
    while s.len() < size {
        s.push_str("DEBUG ");
        s.push_str(&random_text(&mut x, 48));
        s.push('\n');
    }
    s
}

fn json_input(kind: Kind, size: usize) -> String {
    match kind {
        Kind::Repetitive => json_repetitive(size),
        Kind::Incompressible => json_incompressible(size),
    }
}

fn table_input(kind: Kind, size: usize) -> String {
    match kind {
        Kind::Repetitive => json_repetitive(size),
        Kind::Incompressible => table_incompressible(size),
    }
}

fn lines_input(kind: Kind, size: usize) -> String {
    match kind {
        Kind::Repetitive => lines_repetitive(size),
        Kind::Incompressible => lines_incompressible(size),
    }
}

fn ctx(json_minify: bool, json_table: bool, exact_line_runs: bool) -> CompressionContext<'static> {
    CompressionContext {
        mode: Mode::Safe,
        json_minify,
        json_table,
        exact_line_runs,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

fn output_len(input: &str, ctx: &CompressionContext) -> usize {
    compress(input, ctx).output.len()
}

fn full_compress(input: &str) -> usize {
    let counter = EstimateCounter;
    let ctx = CompressionContext {
        token_counter: Some(&counter),
        limits: deslag_core::Limits::default(),
        ..ctx(true, true, true)
    };
    output_len(input, &ctx)
}

struct Stage {
    name: &'static str,
    input: fn(Kind, usize) -> String,
    run: fn(&str) -> usize,
}

const STAGES: [Stage; 5] = [
    Stage {
        name: "minify scan",
        input: json_input,
        run: |s| output_len(s, &ctx(true, false, false)),
    },
    Stage {
        name: "table-v1 encode",
        input: table_input,
        run: |s| output_len(s, &ctx(false, true, false)),
    },
    Stage {
        name: "exact-line-runs",
        input: lines_input,
        run: |s| output_len(s, &ctx(false, false, true)),
    },
    Stage {
        name: "token estimate",
        input: json_input,
        run: |s| EstimateCounter.count(s).count as usize,
    },
    Stage {
        name: "full compress + gain",
        input: json_input,
        run: full_compress,
    },
];

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn bench_stage(stage: &Stage, kind: Kind, input: &str) {
    let samples = (64 * 1024 * 1024 / input.len()).clamp(100, 2000);
    for _ in 0..WARMUP {
        black_box((stage.run)(black_box(input)));
    }
    // One measured call for heap numbers; the timing loop below stays free of counter reads.
    let (count0, bytes0, base) = (
        COUNT.load(Relaxed),
        BYTES.load(Relaxed),
        CURRENT.load(Relaxed),
    );
    PEAK.store(base, Relaxed);
    let out = black_box((stage.run)(black_box(input)));
    let allocs = COUNT.load(Relaxed) - count0;
    let bytes = BYTES.load(Relaxed) - bytes0;
    let peak = PEAK.load(Relaxed).saturating_sub(base);

    let mut times: Vec<Duration> = (0..samples)
        .map(|_| {
            let start = Instant::now();
            black_box((stage.run)(black_box(input)));
            start.elapsed()
        })
        .collect();
    times.sort();
    let p50 = percentile(&times, 0.50);
    let mib_s = input.len() as f64 / (1024.0 * 1024.0) / p50.as_secs_f64();
    println!(
        "| {} | {} | {} | {out} | {samples} | {:?} | {:?} | {:?} | {mib_s:.0} | {allocs} | {bytes} | {peak} |",
        stage.name,
        kind.name(),
        input.len(),
        p50,
        percentile(&times, 0.95),
        percentile(&times, 0.99),
    );
}

fn bench_concurrency(label: &str, threads: usize, exec: &BoundedExecutor, input: &Arc<String>) {
    let (completed, busy, expired, panicked) = (
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let start = Instant::now();
    let workers: Vec<_> = (0..threads)
        .map(|_| {
            let (exec, input) = (exec.clone(), Arc::clone(input));
            let (completed, busy, expired, panicked) = (
                Arc::clone(&completed),
                Arc::clone(&busy),
                Arc::clone(&expired),
                Arc::clone(&panicked),
            );
            std::thread::spawn(move || {
                for _ in 0..CALLS_PER_THREAD {
                    let input = Arc::clone(&input);
                    let counter = match exec.run(move || full_compress(&input)) {
                        Ok(_) => &completed,
                        Err(Rejected::Busy) => &busy,
                        Err(Rejected::BudgetExceeded) => &expired,
                        Err(Rejected::Panicked) => &panicked,
                    };
                    counter.fetch_add(1, Relaxed);
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    // Expired jobs may still hold permits; the next row must start from an idle executor.
    while exec.in_flight() != 0 {
        std::thread::yield_now();
    }
    let wall = start.elapsed();
    let total = (threads * CALLS_PER_THREAD) as f64;
    let rate = |n: &AtomicUsize| n.load(Relaxed) as f64 / total * 100.0;
    println!(
        "| {label} | {threads} | {} | {:.0} | {} | {:.1}% | {:.1}% | {:.1}% |",
        exec.max_parallel_jobs(),
        completed.load(Relaxed) as f64 / wall.as_secs_f64(),
        completed.load(Relaxed),
        rate(&busy),
        rate(&expired),
        rate(&panicked),
    );
}

fn main() {
    println!("### deslag-core stages (concurrency 1, warmup {WARMUP})\n");
    println!(
        "Corpus `{CORPUS_VERSION}`. Tokenizer: method `estimate`, `{}`. Samples per row: 64 MiB divided by the input size, clamped to 100..=2000. \
         Allocations, bytes and peak heap come from one call after warmup. A growing realloc counts as one allocation of the added bytes.\n",
        EstimateCounter::TOKENIZER
    );
    println!(
        "| stage | input | bytes | output (bytes; tokens for token estimate) | samples | p50 | p95 | p99 | MiB/s at p50 | allocs/call | alloc bytes/call | peak heap bytes/call |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    for stage in &STAGES {
        for kib in SIZES_KIB {
            for kind in [Kind::Repetitive, Kind::Incompressible] {
                bench_stage(stage, kind, &(stage.input)(kind, kib * 1024));
            }
        }
    }
    println!();

    let input = Arc::new(json_repetitive(CONCURRENCY_INPUT_KIB * 1024));
    let default = BoundedExecutor::default();
    println!(
        "### deslag-core bounded execution ({CONCURRENCY_INPUT_KIB} KiB repetitive, full compress, {CALLS_PER_THREAD} calls per thread)\n"
    );
    println!(
        "Default limits: {} jobs, {:?} budget.\n",
        default.max_parallel_jobs(),
        default.processing_budget()
    );
    println!(
        "| limits | concurrency | max jobs | completed/s | completed | busy | budget_exceeded | job_panicked |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for threads in CONCURRENCY {
        bench_concurrency("default", threads, &default, &input);
        let widened = BoundedExecutor::new(threads, default.processing_budget()).unwrap();
        bench_concurrency("max jobs = concurrency", threads, &widened, &input);
    }
    println!();
}
