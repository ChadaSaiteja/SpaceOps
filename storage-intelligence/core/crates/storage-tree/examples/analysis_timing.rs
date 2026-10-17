//! Times the post-scan analyses (developer artifacts, apps, leftovers) on a real folder.
//! Read-only: scans the folder and runs detection; nothing is cleaned.
//!
//! cargo run --release -p storage-tree --example analysis_timing -- <folder>

use std::time::{Duration, Instant};
use storage_tree::{app_manager, developer, StorageTree};

fn main() {
    let root = std::env::args()
        .nth(1)
        .expect("usage: analysis_timing <folder>");
    let cancel = common::CancellationToken::new();

    let t = Instant::now();
    let (events, _summary) = scanner::scan_with_options(
        std::path::Path::new(&root),
        &cancel,
        Duration::from_secs(1),
        None,
        &|_| {},
    )
    .expect("scan failed");
    println!(
        "scan            {:>8.2} s ({} events)",
        t.elapsed().as_secs_f64(),
        events.len()
    );

    let t = Instant::now();
    let tree = StorageTree::build(events).expect("build failed");
    println!(
        "tree build      {:>8.2} s ({} nodes)",
        t.elapsed().as_secs_f64(),
        tree.node_count()
    );

    let t = Instant::now();
    let junk = tree.detect_cleanup_candidates();
    println!(
        "cleanup rules   {:>8.2} s ({} candidates)",
        t.elapsed().as_secs_f64(),
        junk.len()
    );
    for c in &junk {
        println!(
            "    {:<34} {:>14} bytes {:>8} files {:>7} targets",
            c.name,
            c.total_bytes,
            c.file_count,
            c.target_paths.len()
        );
    }

    let t = Instant::now();
    let arts = developer::detect_dev_artifacts(Some(&tree));
    let dormant = arts.iter().filter(|a| a.is_dormant).count();
    println!(
        "dev artifacts   {:>8.2} s ({} found, {} dormant)",
        t.elapsed().as_secs_f64(),
        arts.len(),
        dormant
    );

    let t = Instant::now();
    let apps = app_manager::discover_win32_apps(Some(&tree));
    println!(
        "apps            {:>8.2} s ({} apps)",
        t.elapsed().as_secs_f64(),
        apps.len()
    );

    let t = Instant::now();
    let left = app_manager::find_app_leftovers(&apps, Some(&tree));
    println!(
        "leftovers       {:>8.2} s ({} found)",
        t.elapsed().as_secs_f64(),
        left.len()
    );
}
