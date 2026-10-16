//! Benchmark harness for Sub-phase 2.9 (performance validation, PRD SS17: measure,
//! don't guess). Builds a synthetic tree, then times the same `scan()` code path once
//! with rayon's default (parallel) thread pool and once forced to a single thread, to
//! isolate the actual speedup contributed by parallelism (Sub-phase 2.5).
//!
//! Run with: cargo run --release --example benchmark -- <file_count>
//! Defaults to 200,000 files if no argument is given (kept below the design doc's
//! ~1,000,000-file target to keep this runnable in a normal dev session; the full
//! 1M-file target should be validated on real hardware before shipping).

use common::CancellationToken;
use std::path::Path;
use std::time::{Duration, Instant};

fn build_synthetic_tree(root: &Path, file_count: u64) {
    let files_per_dir = 100u64;
    let dirs = file_count.div_ceil(files_per_dir);
    let content = vec![b'x'; 128];

    for d in 0..dirs {
        let dir = root.join(format!("d{d}"));
        std::fs::create_dir_all(&dir).expect("create synthetic dir");
        let remaining = file_count - d * files_per_dir;
        let count_here = remaining.min(files_per_dir);
        for f in 0..count_here {
            std::fs::write(dir.join(format!("f{f}.bin")), &content).expect("write synthetic file");
        }
    }
}

fn run_scan(root: &Path) -> Duration {
    let start = Instant::now();
    let result = scanner::scan(
        root,
        &CancellationToken::new(),
        Duration::from_millis(200),
        &|_| {},
    );
    let elapsed = start.elapsed();
    let (_, summary) = result.expect("scan should succeed on synthetic tree");
    println!(
        "    files={} dirs={} bytes={} inaccessible={} elapsed={:?}",
        summary.total_files,
        summary.total_dirs,
        summary.total_size,
        summary.inaccessible_count,
        elapsed
    );
    elapsed
}

fn main() {
    let arg = std::env::args().nth(1);

    // Real-folder mode: `benchmark <existing directory>` runs a read-only parallel scan.
    if let Some(dir) = arg.as_deref().map(Path::new).filter(|p| p.is_dir()) {
        // Optional 2nd arg: number of in-process repetitions (default 1). Reports
        // min/median so OS cache and background-load noise can be filtered out.
        let runs: usize = std::env::args()
            .nth(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
            .max(1);
        println!("Read-only scan of {} ({runs} runs)", dir.display());
        let mut times: Vec<Duration> = (0..runs).map(|_| run_scan(dir)).collect();
        times.sort();
        println!(
            "SUMMARY min={:?} median={:?} max={:?}",
            times[0],
            times[times.len() / 2],
            times[times.len() - 1]
        );
        return;
    }

    let file_count: u64 = arg.and_then(|s| s.parse().ok()).unwrap_or(200_000);

    println!("Building synthetic tree with {file_count} files...");
    let build_start = Instant::now();
    let tmp = tempfile::tempdir().expect("create temp root");
    build_synthetic_tree(tmp.path(), file_count);
    println!("  tree built in {:?}", build_start.elapsed());

    println!(
        "\nParallel scan (rayon default thread pool, {} threads available):",
        rayon::current_num_threads()
    );
    let parallel_elapsed = run_scan(tmp.path());

    println!("\nSingle-threaded scan (rayon pool forced to 1 thread):");
    let single_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("build single-threaded rayon pool");
    let single_elapsed = single_pool.install(|| run_scan(tmp.path()));

    let speedup = single_elapsed.as_secs_f64() / parallel_elapsed.as_secs_f64();
    println!("\nSpeedup from parallelism: {speedup:.2}x (single={single_elapsed:?}, parallel={parallel_elapsed:?})");
}
