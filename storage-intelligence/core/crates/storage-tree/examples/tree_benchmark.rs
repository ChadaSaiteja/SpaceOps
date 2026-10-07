use common::{DirMetadata, FileAttributeFlags, FileMetadata, NodeId, ScanEvent};
use std::ffi::OsString;
use std::time::{Duration, Instant};
use storage_tree::StorageTree;

#[repr(C)]
#[allow(non_snake_case)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[cfg(windows)]
extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn K32GetProcessMemoryInfo(
        process: *mut std::ffi::c_void,
        counters: *mut ProcessMemoryCounters,
        cb: u32,
    ) -> i32;
}

fn get_working_set_bytes() -> usize {
    #[cfg(windows)]
    unsafe {
        let mut counters: ProcessMemoryCounters = std::mem::zeroed();
        counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;
        let proc = GetCurrentProcess();
        if K32GetProcessMemoryInfo(proc, &mut counters, counters.cb) != 0 {
            counters.working_set_size
        } else {
            0
        }
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Generates a realistic synthetic scan event stream with approximately `target_nodes` nodes.
/// Includes one large directory containing `special_large_dir_count` children.
fn generate_synthetic_events(target_nodes: usize, special_large_dir_count: usize) -> (Vec<ScanEvent>, u64) {
    let mut events = Vec::with_capacity(target_nodes * 2);
    let mut current_scanner_id = 0u64;

    // Root directory
    let root_id = NodeId(current_scanner_id);
    current_scanner_id += 1;

    events.push(ScanEvent::EnteredDirectory {
        id: root_id,
        parent_id: None,
        meta: DirMetadata {
            name: OsString::from(r"C:\BenchmarkDrive"),
            is_reparse_point: false,
        },
    });

    // Special directory with 10K children
    let special_dir_id = NodeId(current_scanner_id);
    current_scanner_id += 1;

    events.push(ScanEvent::EnteredDirectory {
        id: special_dir_id,
        parent_id: Some(root_id),
        meta: DirMetadata {
            name: OsString::from("LargeDir10K"),
            is_reparse_point: false,
        },
    });

    let mut special_total_size = 0u64;
    for i in 0..special_large_dir_count {
        let size = ((i % 1000) as u64 + 1) * 1024;
        special_total_size += size;
        events.push(ScanEvent::FileFound {
            parent_id: special_dir_id,
            meta: FileMetadata {
                name: OsString::from(format!("large_file_{i:05}.bin")),
                size,
                extension: Some(String::from("bin")),
                modified: None,
                created: None,
                attributes: FileAttributeFlags::default(),
            },
        });
    }

    events.push(ScanEvent::DirectoryComplete {
        id: special_dir_id,
        total_size: special_total_size,
        file_count: special_large_dir_count as u64,
        dir_count: 0,
    });

    // Remaining nodes distributed across nested folders
    let remaining_nodes = target_nodes.saturating_sub(special_large_dir_count + 2);
    let files_per_dir = 50;
    let num_dirs = (remaining_nodes / (files_per_dir + 1)).max(1);

    let mut root_total_size = special_total_size;
    let mut root_file_count = special_large_dir_count as u64;
    let root_dir_count = 1u64 + num_dirs as u64;

    for d in 0..num_dirs {
        let dir_id = NodeId(current_scanner_id);
        current_scanner_id += 1;
        let dir_name = format!("Folder_{d:04}");

        events.push(ScanEvent::EnteredDirectory {
            id: dir_id,
            parent_id: Some(root_id),
            meta: DirMetadata {
                name: OsString::from(dir_name),
                is_reparse_point: false,
            },
        });

        let mut dir_size = 0u64;
        for f in 0..files_per_dir {
            let size = ((f * 137) % 50000 + 100) as u64;
            dir_size += size;
            events.push(ScanEvent::FileFound {
                parent_id: dir_id,
                meta: FileMetadata {
                    name: OsString::from(format!("file_{d}_{f}.txt")),
                    size,
                    extension: Some(String::from("txt")),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            });
        }

        events.push(ScanEvent::DirectoryComplete {
            id: dir_id,
            total_size: dir_size,
            file_count: files_per_dir as u64,
            dir_count: 0,
        });

        root_total_size += dir_size;
        root_file_count += files_per_dir as u64;
    }

    events.push(ScanEvent::DirectoryComplete {
        id: root_id,
        total_size: root_total_size,
        file_count: root_file_count,
        dir_count: root_dir_count,
    });

    (events, special_dir_id.0)
}

struct BenchmarkResult {
    node_count: usize,
    build_duration: Duration,
    node_lookup_avg_nanos: f64,
    children_query_micros: f64,
    top_files_micros: f64,
    _rss_mb: f64,
    delta_rss_mb: f64,
}

fn run_benchmark(target_nodes: usize) -> BenchmarkResult {
    println!("\n========================================================");
    println!("Generating synthetic stream for ~{target_nodes} nodes...");
    let (events, special_dir_raw_id) = generate_synthetic_events(target_nodes, 10_000);
    println!("Event stream generated: {} events", events.len());

    let rss_before = get_working_set_bytes();

    println!("Building StorageTree...");
    let start_build = Instant::now();
    let tree = StorageTree::build(events).expect("Tree build failed");
    let build_duration = start_build.elapsed();

    let rss_after = get_working_set_bytes();
    let actual_nodes = tree.node_count();
    let delta_rss_mb = (rss_after.saturating_sub(rss_before)) as f64 / (1024.0 * 1024.0);
    let total_rss_mb = rss_after as f64 / (1024.0 * 1024.0);

    println!("Tree built in {build_duration:.3?}. Actual node count: {actual_nodes}");
    println!("Memory: delta RSS = {delta_rss_mb:.1} MB, total process RSS = {total_rss_mb:.1} MB");

    // Benchmark node() lookup latency (100,000 lookups across the tree)
    let lookups = 100_000usize;
    let step = (actual_nodes / lookups).max(1);
    let start_lookup = Instant::now();
    let mut found = 0;
    for i in (0..actual_nodes).step_by(step) {
        if tree.node(NodeId(i as u64)).is_some() {
            found += 1;
        }
    }
    let lookup_duration = start_lookup.elapsed();
    let avg_lookup_nanos = lookup_duration.as_nanos() as f64 / found.max(1) as f64;
    println!("node() lookup: {found} lookups in {lookup_duration:.3?} (avg: {avg_lookup_nanos:.1} ns/lookup)");

    // Benchmark children() query on the 10K-child directory
    // Find the special directory in tree
    let special_node_id = NodeId(special_dir_raw_id);
    let start_children = Instant::now();
    let children = tree.children(special_node_id).expect("Special dir missing");
    let children_duration = start_children.elapsed();
    let children_query_micros = children_duration.as_secs_f64() * 1_000_000.0;
    println!(
        "children() query on 10K-child dir: {} children in {children_duration:.3?} ({children_query_micros:.1} µs)",
        children.len()
    );

    // Benchmark top_files_by_size(root, 100)
    let start_top = Instant::now();
    let top_files = tree.top_files_by_size(tree.root(), 100);
    let top_duration = start_top.elapsed();
    let top_files_micros = top_duration.as_secs_f64() * 1_000_000.0;
    println!(
        "top_files_by_size(root, 100): {} files in {top_duration:.3?} ({top_files_micros:.1} µs)",
        top_files.len()
    );

    BenchmarkResult {
        node_count: actual_nodes,
        build_duration,
        node_lookup_avg_nanos: avg_lookup_nanos,
        children_query_micros,
        top_files_micros,
        _rss_mb: total_rss_mb,
        delta_rss_mb,
    }
}

fn main() {
    println!("========================================================");
    println!("     Storage Tree Performance Benchmark (Phase 3)");
    println!("========================================================");

    let sizes = [200_000, 500_000, 1_000_000];
    let mut results = Vec::new();

    for &size in &sizes {
        results.push(run_benchmark(size));
    }

    println!("\n========================================================");
    println!("                  SUMMARY TABLE");
    println!("========================================================");
    println!(
        "{:<10} | {:<12} | {:<14} | {:<16} | {:<16} | {:<12} | {:<10}",
        "Nodes", "Build Time", "node() Lookup", "children(10K)", "top_files(100)", "Tree RSS", "Status"
    );
    println!("{:-<10}-+-{:-<12}-+-{:-<14}-+-{:-<16}-+-{:-<16}-+-{:-<12}-+-{:-<10}", "", "", "", "", "", "", "");

    for r in &results {
        let build_ok = r.build_duration <= Duration::from_millis(2000);
        let lookup_ok = r.node_lookup_avg_nanos < 1000.0; // < 1 µs
        let children_ok = r.children_query_micros < 1000.0; // < 1 ms
        let top_ok = r.top_files_micros < 50_000.0; // < 50 ms
        let mem_ok = r.delta_rss_mb < 300.0;

        let all_ok = build_ok && lookup_ok && children_ok && top_ok && mem_ok;

        println!(
            "{:<10} | {:<10.2?} | {:<11.1} ns | {:<13.1} µs | {:<13.1} µs | {:<9.1} MB | {:<10}",
            r.node_count,
            r.build_duration,
            r.node_lookup_avg_nanos,
            r.children_query_micros,
            r.top_files_micros,
            r.delta_rss_mb,
            if all_ok { "PASS" } else { "CHECK" }
        );
    }

    println!("\nTarget Comparison (1,000,000 nodes):");
    let r1m = results.last().unwrap();
    println!("  Build time:        {:?} (target: <= 2.0 s) -> {}", r1m.build_duration, if r1m.build_duration <= Duration::from_millis(2000) { "MET" } else { "MISSED" });
    println!("  node() lookup:     {:.1} ns (target: < 1000 ns) -> {}", r1m.node_lookup_avg_nanos, if r1m.node_lookup_avg_nanos < 1000.0 { "MET" } else { "MISSED" });
    println!("  children(10K):     {:.1} µs (target: < 1000 µs) -> {}", r1m.children_query_micros, if r1m.children_query_micros < 1000.0 { "MET" } else { "MISSED" });
    println!("  top_files(100):    {:.1} ms (target: < 50 ms) -> {}", r1m.top_files_micros / 1000.0, if r1m.top_files_micros < 50_000.0 { "MET" } else { "MISSED" });
    println!("  Tree delta RSS:    {:.1} MB (target: < 300 MB) -> {}", r1m.delta_rss_mb, if r1m.delta_rss_mb < 300.0 { "MET" } else { "MISSED" });
}
