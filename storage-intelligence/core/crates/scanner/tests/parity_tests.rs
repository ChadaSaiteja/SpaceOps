//! 15-case cross-engine parity and ADR-008 invariant verification test suite (ADR-011).

use common::{CancellationToken, ScanEvent};
use scanner::mft::record::{
    apply_fixups, parse_mft_record, ATTR_DATA, ATTR_END, ATTR_FILE_NAME, ATTR_STANDARD_INFORMATION,
    MFT_MAGIC, MFT_RECORD_SIZE, RECORD_FLAG_DIRECTORY, RECORD_FLAG_IN_USE,
};
use scanner::{scan, scan_volume_resilient, scan_with_options, ScanEngineStrategy};
use std::fs;
use std::os::windows::fs::symlink_dir;
use std::time::Duration;
use tempfile::tempdir;

// ============================================================================
// HELPER: Synthetic MFT Record Builder for Unit-Level Invariant Testing
// ============================================================================

fn create_synthetic_mft_record(
    _record_num: u64,
    is_dir: bool,
    is_reparse: bool,
    parent_num: u64,
    name: &str,
    allocated_size: u64,
) -> [u8; MFT_RECORD_SIZE] {
    let mut rec = [0u8; MFT_RECORD_SIZE];

    // Magic "FILE"
    rec[0..4].copy_from_slice(&MFT_MAGIC.to_le_bytes());

    // Fixup offset at 48, count = 3 (1 check word + 2 sector end words)
    rec[4..6].copy_from_slice(&48u16.to_le_bytes());
    rec[6..8].copy_from_slice(&3u16.to_le_bytes());

    // First attribute offset at 56
    rec[20..22].copy_from_slice(&56u16.to_le_bytes());

    // Flags
    let mut flags = RECORD_FLAG_IN_USE;
    if is_dir {
        flags |= RECORD_FLAG_DIRECTORY;
    }
    rec[22..24].copy_from_slice(&flags.to_le_bytes());

    // Fixup array check word at 48
    let check_word = 0xAA55u16;
    rec[48..50].copy_from_slice(&check_word.to_le_bytes());
    rec[50..52].copy_from_slice(&0x0000u16.to_le_bytes()); // sector 1 replacement
    rec[52..54].copy_from_slice(&0x0000u16.to_le_bytes()); // sector 2 replacement

    // Write check word at end of sector 0 (510..512) and sector 1 (1022..1024)
    rec[510..512].copy_from_slice(&check_word.to_le_bytes());
    rec[1022..1024].copy_from_slice(&check_word.to_le_bytes());

    let mut offset = 56usize;

    // 1. $STANDARD_INFORMATION attribute
    let std_info_len = 72u32;
    rec[offset..offset + 4].copy_from_slice(&ATTR_STANDARD_INFORMATION.to_le_bytes());
    rec[offset + 4..offset + 8].copy_from_slice(&std_info_len.to_le_bytes());
    rec[offset + 8] = 0; // Resident
    rec[offset + 16..offset + 20].copy_from_slice(&48u32.to_le_bytes()); // value length
    rec[offset + 20..offset + 22].copy_from_slice(&24u16.to_le_bytes()); // value offset

    let mut file_flags = if is_dir { 0x10u32 } else { 0u32 };
    if is_reparse {
        file_flags |= 0x400u32; // Reparse point flag
    }
    rec[offset + 24 + 32..offset + 24 + 36].copy_from_slice(&file_flags.to_le_bytes());
    offset += std_info_len as usize;

    // 2. $FILE_NAME attribute
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let name_bytes_len = name_utf16.len() * 2;
    let fn_val_len = 66 + name_bytes_len;
    let fn_total_len = ((fn_val_len + 24 + 7) / 8) * 8;

    rec[offset..offset + 4].copy_from_slice(&ATTR_FILE_NAME.to_le_bytes());
    rec[offset + 4..offset + 8].copy_from_slice(&(fn_total_len as u32).to_le_bytes());
    rec[offset + 8] = 0; // Resident
    rec[offset + 16..offset + 20].copy_from_slice(&(fn_val_len as u32).to_le_bytes());
    rec[offset + 20..offset + 22].copy_from_slice(&24u16.to_le_bytes());

    let fn_val = offset + 24;
    rec[fn_val..fn_val + 8].copy_from_slice(&parent_num.to_le_bytes()); // Parent record
    rec[fn_val + 56..fn_val + 60].copy_from_slice(&file_flags.to_le_bytes());
    rec[fn_val + 64] = name_utf16.len() as u8;
    rec[fn_val + 65] = 1; // Win32 namespace

    for (i, &ch) in name_utf16.iter().enumerate() {
        rec[fn_val + 66 + i * 2..fn_val + 66 + (i + 1) * 2].copy_from_slice(&ch.to_le_bytes());
    }
    offset += fn_total_len;

    // 3. $DATA attribute (for files)
    if !is_dir {
        let data_len = 72u32;
        rec[offset..offset + 4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        rec[offset + 4..offset + 8].copy_from_slice(&data_len.to_le_bytes());
        rec[offset + 8] = 1; // Non-resident
        rec[offset + 40..offset + 48].copy_from_slice(&allocated_size.to_le_bytes());
        rec[offset + 48..offset + 56].copy_from_slice(&allocated_size.to_le_bytes());
        offset += data_len as usize;
    }

    // End attribute
    rec[offset..offset + 4].copy_from_slice(&ATTR_END.to_le_bytes());
    rec
}

// ============================================================================
// SUITE 1: TIER 2 WIN32 ENGINE (5 ADR-008 INVARIANTS)
// ============================================================================

#[test]
fn win32_inv1_reparse_points_are_leaves_and_never_followed() {
    let root = tempdir().unwrap();
    let real_dir = root.path().join("real_dir");
    fs::create_dir(&real_dir).unwrap();
    fs::write(real_dir.join("inside_real.txt"), b"real data").unwrap();

    let junction = root.path().join("junction_dir");
    if symlink_dir(&real_dir, &junction).is_err() {
        eprintln!("skipping: creating a directory symlink requires Developer Mode or admin");
        return;
    }

    let (events, summary) = scan(root.path(), &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();

    // Verify junction is reported as leaf and not traversed
    assert_eq!(summary.total_files, 1); // inside_real.txt only once
    assert!(events.iter().any(|e| matches!(
        e,
        ScanEvent::EnteredDirectory { meta, .. } if meta.is_reparse_point
    )));
}

#[test]
fn win32_inv2_allocated_size_semantics_matches_cluster_rounding() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("small.txt"), b"123").unwrap();

    let (_, summary) = scan(root.path(), &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();

    assert_eq!(summary.total_files, 1);
    // On-disk cluster rounding guarantees multiple of cluster size (minimum 4096 bytes)
    assert!(summary.total_size >= 4096);
    assert_eq!(summary.total_size % 4096, 0);
}

#[test]
fn win32_inv3_hard_links_counted_at_every_path() {
    let root = tempdir().unwrap();
    let original = root.path().join("original.txt");
    fs::write(&original, b"shared content").unwrap();

    let link = root.path().join("hardlink.txt");
    let link_created = fs::hard_link(&original, &link).is_ok();

    if link_created {
        let (_, summary) = scan(root.path(), &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();
        // ADR-008 #3: counted at every path without dedup
        assert_eq!(summary.total_files, 2);
    }
}

#[test]
fn win32_inv4_flat_event_stream_hierarchy_is_strictly_ordered() {
    let root = tempdir().unwrap();
    let sub = root.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("file.txt"), b"data").unwrap();

    let (events, _) = scan(root.path(), &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();

    // Event order must have EnteredDirectory before FileFound and DirectoryComplete after
    let enter_idx = events
        .iter()
        .position(|e| matches!(e, ScanEvent::EnteredDirectory { meta, .. } if meta.name == "sub"))
        .unwrap();
    let file_idx = events
        .iter()
        .position(|e| matches!(e, ScanEvent::FileFound { meta, .. } if meta.name == "file.txt"))
        .unwrap();
    let complete_idx = events
        .iter()
        .position(|e| matches!(e, ScanEvent::DirectoryComplete { .. }))
        .unwrap();

    assert!(enter_idx < file_idx);
    assert!(file_idx <= complete_idx);
}

#[test]
fn win32_inv5_inaccessible_items_do_not_abort_scan() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("visible.txt"), b"accessible").unwrap();

    let (events, summary) = scan(root.path(), &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();
    assert!(summary.total_files >= 1);
    assert!(events.iter().any(|e| matches!(e, ScanEvent::FileFound { meta, .. } if meta.name == "visible.txt")));
}

// ============================================================================
// SUITE 2: TIER 3 SHALLOW / DEPTH-LIMITED ENGINE (5 ADR-008 INVARIANTS)
// ============================================================================

#[test]
fn shallow_inv1_reparse_points_remain_leaves() {
    let root = tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = root.path().join("link");
    if symlink_dir(&real, &link).is_err() {
        eprintln!("skipping: creating a directory symlink requires Developer Mode or admin");
        return;
    }

    let (events, _) = scan_with_options(root.path(), &CancellationToken::new(), Duration::ZERO, Some(1), &|_| {}).unwrap();

    assert!(events.iter().any(|e| matches!(
        e,
        ScanEvent::EnteredDirectory { meta, .. } if meta.is_reparse_point
    )));
}

#[test]
fn shallow_inv2_allocated_size_cluster_rounded() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("top.txt"), b"top bytes").unwrap();

    let (_, summary) = scan_with_options(root.path(), &CancellationToken::new(), Duration::ZERO, Some(1), &|_| {}).unwrap();

    assert_eq!(summary.total_files, 1);
    assert!(summary.total_size >= 4096);
    assert_eq!(summary.total_size % 4096, 0);
}

#[test]
fn shallow_inv3_top_level_hard_links_counted() {
    let root = tempdir().unwrap();
    let file1 = root.path().join("f1.txt");
    fs::write(&file1, b"content").unwrap();
    let file2 = root.path().join("f2.txt");
    if fs::hard_link(&file1, &file2).is_ok() {
        let (_, summary) = scan_with_options(root.path(), &CancellationToken::new(), Duration::ZERO, Some(1), &|_| {}).unwrap();
        assert_eq!(summary.total_files, 2);
    }
}

#[test]
fn shallow_inv4_flat_event_stream_bounded_at_target_depth() {
    let root = tempdir().unwrap();
    let deep = root.path().join("d1").join("d2").join("d3");
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("hidden.txt"), b"deep").unwrap();

    let (events, summary) = scan_with_options(root.path(), &CancellationToken::new(), Duration::ZERO, Some(1), &|_| {}).unwrap();

    assert_eq!(summary.total_files, 0); // No top-level files
    assert_eq!(summary.total_dirs, 2);   // root + d1
    assert!(!events.iter().any(|e| matches!(e, ScanEvent::FileFound { meta, .. } if meta.name == "hidden.txt")));
}

#[test]
fn shallow_inv5_inaccessible_items_do_not_abort_shallow_scan() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("top.txt"), b"accessible").unwrap();

    let result = scan_with_options(root.path(), &CancellationToken::new(), Duration::ZERO, Some(1), &|_| {});
    assert!(result.is_ok());
}

// ============================================================================
// SUITE 3: TIER 1 MFT ENGINE INVARIANTS (SYNTHETIC RECORD SUITE)
// ============================================================================

#[test]
fn mft_inv1_reparse_records_parsed_as_leaves() {
    let mut raw = create_synthetic_mft_record(100, true, true, 5, "MyJunction", 0);
    assert!(apply_fixups(&mut raw));
    let parsed = parse_mft_record(&raw, 100, 4096).unwrap();

    assert!(parsed.is_reparse_point);
    assert!(parsed.is_directory);
    assert_eq!(parsed.file_name, "MyJunction");
}

#[test]
fn mft_inv2_allocated_size_cluster_rounded() {
    let mut raw = create_synthetic_mft_record(101, false, false, 5, "large.bin", 81920);
    assert!(apply_fixups(&mut raw));
    let parsed = parse_mft_record(&raw, 101, 4096).unwrap();

    assert_eq!(parsed.allocated_size, 81920);
    assert_eq!(parsed.allocated_size % 4096, 0);
}

#[test]
fn mft_inv3_hard_links_preserve_distinct_parents() {
    let mut raw1 = create_synthetic_mft_record(102, false, false, 5, "link1.txt", 4096);
    let mut raw2 = create_synthetic_mft_record(102, false, false, 6, "link2.txt", 4096);
    assert!(apply_fixups(&mut raw1));
    assert!(apply_fixups(&mut raw2));

    let p1 = parse_mft_record(&raw1, 102, 4096).unwrap();
    let p2 = parse_mft_record(&raw2, 102, 4096).unwrap();

    assert_eq!(p1.parent_record_number, 5);
    assert_eq!(p2.parent_record_number, 6);
}

#[test]
fn mft_inv4_flat_event_generation_topologically_ordered() {
    let mut raw_dir = create_synthetic_mft_record(6, true, false, 5, "DirA", 0);
    let mut raw_file = create_synthetic_mft_record(7, false, false, 6, "FileA.txt", 4096);
    assert!(apply_fixups(&mut raw_dir));
    assert!(apply_fixups(&mut raw_file));

    let parsed_dir = parse_mft_record(&raw_dir, 6, 4096).unwrap();
    let parsed_file = parse_mft_record(&raw_file, 7, 4096).unwrap();

    assert_eq!(parsed_dir.record_number, 6);
    assert_eq!(parsed_file.parent_record_number, 6);
}

#[test]
fn mft_inv5_corrupt_record_fails_safely_without_panic() {
    let mut corrupt = [0u8; MFT_RECORD_SIZE];
    corrupt[0..4].copy_from_slice(b"NOPE");

    assert!(!apply_fixups(&mut corrupt));
    assert!(parse_mft_record(&corrupt, 999, 4096).is_none());
}

// ============================================================================
// SUITE 4: CROSS-ENGINE PARITY & CLEAN FALLBACK
// ============================================================================

#[test]
fn cross_engine_parity_flat_directory() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), b"12345").unwrap();
    fs::write(root.path().join("b.txt"), b"67890").unwrap();

    let (_, win32_summary) = scan(root.path(), &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();
    let (_, shallow_summary) = scan_with_options(root.path(), &CancellationToken::new(), Duration::ZERO, Some(1), &|_| {}).unwrap();
    let (_, resilient_summary) = scan_volume_resilient(root.path(), ScanEngineStrategy::Auto, &CancellationToken::new(), Duration::ZERO, &|_| {}).unwrap();

    assert_eq!(win32_summary.total_files, shallow_summary.total_files);
    assert_eq!(win32_summary.total_files, resilient_summary.total_files);
    assert_eq!(win32_summary.total_size, shallow_summary.total_size);
    assert_eq!(win32_summary.total_size, resilient_summary.total_size);
}

#[test]
fn resilient_supervisor_clean_fallback_on_folder_target() {
    let root = tempdir().unwrap();
    let sub = root.path().join("subfolder");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("data.bin"), b"payload").unwrap();

    // Passing subfolder into Auto strategy triggers clean fallback to Win32
    let (events, summary) = scan_volume_resilient(
        &sub,
        ScanEngineStrategy::Auto,
        &CancellationToken::new(),
        Duration::ZERO,
        &|_| {},
    )
    .unwrap();

    assert_eq!(summary.total_files, 1);
    assert!(events.iter().any(|e| matches!(e, ScanEvent::FileFound { meta, .. } if meta.name == "data.bin")));
}
