//! `minify_json` latency and throughput by candidate size (PLAN §13.1).
//! No thresholds: results are a measured baseline only.

use std::hint::black_box;
use std::time::{Duration, Instant};

use deslag_core::minify_json;

const SIZES_KIB: [usize; 5] = [2, 16, 64, 256, 1024];
const WARMUP: usize = 10;

/// Pretty-printed records with the same keys: the common case, minifies.
fn repetitive(size: usize) -> String {
    let mut s = String::from("[\n");
    let mut i = 0;
    while s.len() < size {
        s.push_str(&format!(
            "  {{\n    \"id\": {i},\n    \"name\": \"user-{i}\",\n    \"active\": true,\n    \"tags\": [\n      \"a\",\n      \"b\"\n    ]\n  }},\n"
        ));
        i += 1;
    }
    s.push_str("  null\n]\n");
    s
}

/// Already-minified array of pseudo-random strings: a full scan that returns `None`.
fn incompressible(size: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .:/-_";
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut s = String::from("[");
    while s.len() < size {
        s.push('"');
        for _ in 0..32 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            s.push(char::from(ALPHABET[(x % ALPHABET.len() as u64) as usize]));
        }
        s.push_str("\",");
    }
    s.push_str("null]");
    s
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn bench(kind: &str, input: &str) {
    let samples = (64 * 1024 * 1024 / input.len()).clamp(100, 2000);
    for _ in 0..WARMUP {
        black_box(minify_json(black_box(input)));
    }
    let mut times: Vec<Duration> = (0..samples)
        .map(|_| {
            let start = Instant::now();
            black_box(minify_json(black_box(input)));
            start.elapsed()
        })
        .collect();
    times.sort();
    let p50 = percentile(&times, 0.50);
    let mib_s = input.len() as f64 / (1024.0 * 1024.0) / p50.as_secs_f64();
    println!(
        "| {kind} | {} | {} | {samples} | {:?} | {:?} | {:?} | {mib_s:.0} |",
        input.len(),
        minify_json(input).map_or("no", |_| "yes"),
        p50,
        percentile(&times, 0.95),
        percentile(&times, 0.99),
    );
}

fn main() {
    println!("### deslag-core minify_json (concurrency 1, warmup {WARMUP})\n");
    println!("| input | bytes | changed | samples | p50 | p95 | p99 | MiB/s at p50 |");
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for kib in SIZES_KIB {
        bench("repetitive", &repetitive(kib * 1024));
        bench("incompressible", &incompressible(kib * 1024));
    }
    println!();
}
