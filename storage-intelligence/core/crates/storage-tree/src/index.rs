//! Incremental Indexing, SQLite Persistence & USN Journal Synchronization Engine (ADR-016, PRD §10, §22).
//!
//! Provides SQLite database persistence for scanned StorageTree hierarchies, sub-100ms
//! tree hydration on application launch, and sub-50ms incremental updates via NTFS USN Journal
//! and timestamp differential change detection.

use crate::tree::{NodeKind, StorageTree, TreeNode};
use common::{FileAttributeFlags, NodeId};
use rusqlite::{params, Connection, Result as SqlResult};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// High-level metrics for an indexed volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStats {
    pub volume_id: u64,
    pub drive_or_path: String,
    pub node_count: u64,
    pub total_size: u64,
    pub last_scan_time: u64,
    pub last_usn: u64,
}

/// Detailed outcome of an incremental synchronization run (ADR-016 §3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexSyncReport {
    pub nodes_added: u64,
    pub nodes_updated: u64,
    pub nodes_removed: u64,
    pub bytes_delta: i64,
    pub sync_duration_ms: u64,
}

fn pack_attributes(f: &FileAttributeFlags) -> u32 {
    let mut bits = 0u32;
    if f.readonly {
        bits |= 1;
    }
    if f.hidden {
        bits |= 2;
    }
    if f.system {
        bits |= 4;
    }
    if f.reparse_point {
        bits |= 8;
    }
    if f.compressed {
        bits |= 16;
    }
    if f.sparse {
        bits |= 32;
    }
    bits
}

fn unpack_attributes(bits: u32) -> FileAttributeFlags {
    FileAttributeFlags {
        readonly: (bits & 1) != 0,
        hidden: (bits & 2) != 0,
        system: (bits & 4) != 0,
        reparse_point: (bits & 8) != 0,
        compressed: (bits & 16) != 0,
        sparse: (bits & 32) != 0,
    }
}

fn time_to_secs(t: Option<SystemTime>) -> Option<i64> {
    t.and_then(|time| {
        time.duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    })
}

fn secs_to_time(s: Option<i64>) -> Option<SystemTime> {
    s.and_then(|secs| {
        if secs >= 0 {
            Some(UNIX_EPOCH + Duration::from_secs(secs as u64))
        } else {
            None
        }
    })
}

/// Initializes database connection with optimized performance pragmas (ADR-016 §1).
pub fn open_index_db(db_path: &Path) -> SqlResult<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Creates tables and indexes if they do not exist.
pub fn init_schema(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS volumes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            drive_or_path TEXT UNIQUE NOT NULL,
            cluster_size INTEGER NOT NULL DEFAULT 4096,
            last_usn INTEGER NOT NULL DEFAULT 0,
            usn_journal_id INTEGER NOT NULL DEFAULT 0,
            last_scan_time INTEGER NOT NULL,
            total_bytes INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS nodes (
            id INTEGER NOT NULL,
            volume_id INTEGER NOT NULL REFERENCES volumes(id) ON DELETE CASCADE,
            parent_id INTEGER,
            name TEXT NOT NULL,
            size INTEGER NOT NULL,
            file_count INTEGER NOT NULL,
            dir_count INTEGER NOT NULL,
            kind INTEGER NOT NULL,
            extension TEXT,
            modified INTEGER,
            created INTEGER,
            attributes INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (volume_id, id)
        );

        CREATE INDEX IF NOT EXISTS idx_nodes_vol_parent ON nodes(volume_id, parent_id);",
    )
}

/// Saves an in-memory `StorageTree` hierarchy into the SQLite index (ADR-016 §2).
pub fn save_tree_to_db(db_path: &Path, tree: &StorageTree, root_path: &Path) -> SqlResult<()> {
    let mut conn = open_index_db(db_path)?;
    let root_path_str = root_path.to_string_lossy().to_string();

    // Query USN journal parameters if available on NTFS
    let (journal_id, high_usn) =
        if let Some(drive) = scanner::mft::volume::extract_drive_letter(root_path) {
            if let Ok(vol_handle) = scanner::mft::volume::open_volume(drive) {
                if let Ok(info) = scanner::mft::usn::query_usn_journal(vol_handle.raw()) {
                    (info.journal_id as i64, info.next_usn)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

    let total_bytes = tree.node(tree.root()).map(|n| n.size).unwrap_or(0) as i64;
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let tx = conn.transaction()?;

    // 1. Delete previous records for this path
    tx.execute(
        "DELETE FROM volumes WHERE drive_or_path = ?1 COLLATE NOCASE",
        params![root_path_str],
    )?;

    // 2. Insert volume record
    let cluster_size = scanner::winfs::cluster_size(root_path).unwrap_or(4096);
    tx.execute(
        "INSERT INTO volumes (drive_or_path, cluster_size, last_usn, usn_journal_id, last_scan_time, total_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![root_path_str, cluster_size as i64, high_usn, journal_id, now_secs, total_bytes],
    )?;
    let volume_id = tx.last_insert_rowid();

    // 3. Insert all tree nodes in single transaction
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO nodes (id, volume_id, parent_id, name, size, file_count, dir_count, kind, extension, modified, created, attributes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;

        for node in &tree.nodes {
            let kind_int = match node.kind {
                NodeKind::Directory => 0,
                NodeKind::File => 1,
                NodeKind::ReparsePoint => 2,
                NodeKind::Inaccessible => 3,
            };
            let parent_int = node.parent.map(|p| p.0 as i64);
            let attr_int = pack_attributes(&node.attributes) as i64;

            stmt.execute(params![
                node.id.0 as i64,
                volume_id,
                parent_int,
                node.name,
                node.size as i64,
                node.file_count as i64,
                node.dir_count as i64,
                kind_int,
                node.extension.as_deref(),
                time_to_secs(node.modified),
                time_to_secs(node.created),
                attr_int,
            ])?;
        }
    }

    tx.commit()?;
    Ok(())
}

/// Hydrates an in-memory `StorageTree` directly from the local SQLite index in sub-100ms (ADR-016 §2).
pub fn load_tree_from_db(db_path: &Path, root_path: &Path) -> SqlResult<Option<StorageTree>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = open_index_db(db_path)?;
    let root_path_str = root_path.to_string_lossy().to_string();

    let volume_id_opt: Option<i64> = conn
        .query_row(
            "SELECT id FROM volumes WHERE drive_or_path = ?1 COLLATE NOCASE",
            params![root_path_str],
            |row| row.get(0),
        )
        .ok();

    let volume_id = match volume_id_opt {
        Some(vid) => vid,
        None => return Ok(None),
    };

    let mut stmt = conn.prepare(
        "SELECT id, parent_id, name, size, file_count, dir_count, kind, extension, modified, created, attributes
         FROM nodes WHERE volume_id = ?1 ORDER BY id ASC",
    )?;

    let mut rows = stmt.query(params![volume_id])?;
    let mut nodes = Vec::new();
    let mut root_idx = 0;

    while let Some(row) = rows.next()? {
        let id_val: i64 = row.get(0)?;
        let parent_val: Option<i64> = row.get(1)?;
        let name: String = row.get(2)?;
        let size: i64 = row.get(3)?;
        let file_count: i64 = row.get(4)?;
        let dir_count: i64 = row.get(5)?;
        let kind_int: i32 = row.get(6)?;
        let extension: Option<String> = row.get(7)?;
        let modified_secs: Option<i64> = row.get(8)?;
        let created_secs: Option<i64> = row.get(9)?;
        let attr_bits: u32 = row.get(10)?;

        let kind = match kind_int {
            0 => NodeKind::Directory,
            1 => NodeKind::File,
            2 => NodeKind::ReparsePoint,
            _ => NodeKind::Inaccessible,
        };

        if parent_val.is_none() {
            root_idx = nodes.len();
        }

        nodes.push(TreeNode {
            id: NodeId(id_val as u64),
            parent: parent_val.map(|p| NodeId(p as u64)),
            kind,
            name,
            size: size as u64,
            file_count: file_count as u64,
            dir_count: dir_count as u64,
            extension,
            modified: secs_to_time(modified_secs),
            created: secs_to_time(created_secs),
            attributes: unpack_attributes(attr_bits),
        });
    }

    if nodes.is_empty() {
        return Ok(None);
    }

    Ok(Some(StorageTree::reconstruct(nodes, root_idx)))
}

/// Retrieves metrics for an indexed volume.
pub fn get_index_stats(db_path: &Path, root_path: &Path) -> SqlResult<Option<IndexStats>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = open_index_db(db_path)?;
    let root_path_str = root_path.to_string_lossy().to_string();

    let row = conn.query_row(
        "SELECT v.id, v.drive_or_path, v.total_bytes, v.last_scan_time, v.last_usn, COUNT(n.id)
         FROM volumes v
         LEFT JOIN nodes n ON n.volume_id = v.id
         WHERE v.drive_or_path = ?1 COLLATE NOCASE
         GROUP BY v.id",
        params![root_path_str],
        |row| {
            Ok(IndexStats {
                volume_id: row.get::<_, i64>(0)? as u64,
                drive_or_path: row.get(1)?,
                total_size: row.get::<_, i64>(2)? as u64,
                last_scan_time: row.get::<_, i64>(3)? as u64,
                last_usn: row.get::<_, i64>(4)? as u64,
                node_count: row.get::<_, i64>(5)? as u64,
            })
        },
    );

    match row {
        Ok(stats) => Ok(Some(stats)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Deletes an indexed volume and cascades all associated node records.
pub fn delete_indexed_volume(db_path: &Path, root_path: &Path) -> SqlResult<bool> {
    if !db_path.exists() {
        return Ok(false);
    }
    let conn = open_index_db(db_path)?;
    let root_path_str = root_path.to_string_lossy().to_string();

    let affected = conn.execute(
        "DELETE FROM volumes WHERE drive_or_path = ?1 COLLATE NOCASE",
        params![root_path_str],
    )?;

    Ok(affected > 0)
}

/// Performs an incremental synchronization of filesystem changes into the tree and SQLite database (ADR-016 §3).
pub fn sync_tree_incremental(
    db_path: &Path,
    tree: &mut StorageTree,
    root_path: &Path,
) -> SqlResult<IndexSyncReport> {
    let start_time = Instant::now();
    let mut conn = open_index_db(db_path)?;
    let root_path_str = root_path.to_string_lossy().to_string();

    let vol_meta: Option<(i64, i64, i64, i64)> = conn
        .query_row(
            "SELECT id, cluster_size, last_usn, usn_journal_id FROM volumes WHERE drive_or_path = ?1 COLLATE NOCASE",
            params![root_path_str],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok();

    let (volume_id, cluster_size_val, last_usn, journal_id) = match vol_meta {
        Some(v) => v,
        None => {
            // Save first if not indexed yet
            save_tree_to_db(db_path, tree, root_path)?;
            return Ok(IndexSyncReport::default());
        }
    };
    let cluster_size = if cluster_size_val > 0 {
        cluster_size_val as u64
    } else {
        scanner::winfs::cluster_size(root_path).unwrap_or(4096)
    };

    let mut report = IndexSyncReport::default();
    let mut new_high_usn = last_usn;

    // 1. Try NTFS USN Change Journal if elevated and valid
    let usn_synced = if let Some(drive) = scanner::mft::volume::extract_drive_letter(root_path) {
        if let Ok(vol_handle) = scanner::mft::volume::open_volume(drive) {
            if let Ok((records, next_usn)) =
                scanner::mft::usn::read_usn_changes(vol_handle.raw(), journal_id as u64, last_usn)
            {
                new_high_usn = next_usn;
                for record in records {
                    let file_name_str = record.file_name.to_string_lossy();
                    // Match node by name in tree
                    if let Some(node_id) = tree.nodes.iter().position(|n| n.name == file_name_str) {
                        let nid = NodeId(node_id as u64);
                        if (record.reason & scanner::mft::usn::USN_REASON_FILE_DELETE) != 0 {
                            let old_size = tree.nodes[node_id].size;
                            tree.update_node_size(nid, 0, 0);
                            report.nodes_removed += 1;
                            report.bytes_delta -= old_size as i64;
                        } else if (record.reason
                            & (scanner::mft::usn::USN_REASON_DATA_OVERWRITE
                                | scanner::mft::usn::USN_REASON_DATA_EXTEND
                                | scanner::mft::usn::USN_REASON_DATA_TRUNCATION))
                            != 0
                        {
                            if let Some(path) = tree.full_path(nid) {
                                if let Ok(meta) = path.metadata() {
                                    let new_size = scanner::winfs::round_up_to_cluster(
                                        meta.len(),
                                        cluster_size,
                                    );
                                    let old_size = tree.nodes[node_id].size;
                                    let new_mod = meta.modified().ok();
                                    let old_mod = tree.nodes[node_id].modified;

                                    if new_size != old_size || new_mod != old_mod {
                                        if new_size != old_size {
                                            tree.update_node_size(nid, new_size, 1);
                                            report.bytes_delta += new_size as i64 - old_size as i64;
                                        }
                                        tree.nodes[node_id].modified = new_mod;
                                        report.nodes_updated += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    // 2. Fallback: Timestamp differential scan over directory nodes
    if !usn_synced {
        let node_count = tree.nodes.len();
        for i in 0..node_count {
            if tree.nodes[i].kind == NodeKind::Directory {
                let nid = NodeId(i as u64);
                if let Some(dir_path) = tree.full_path(nid) {
                    if let Ok(meta) = dir_path.metadata() {
                        let disk_mod = meta.modified().ok();

                        if let Ok(entries) = std::fs::read_dir(&dir_path) {
                            for entry in entries.flatten() {
                                let p = entry.path();
                                if p.is_file() {
                                    let fname = entry.file_name().to_string_lossy().to_string();
                                    let child_id_opt = tree.children[i]
                                        .iter()
                                        .find(|&&cid| tree.nodes[cid.0 as usize].name == fname)
                                        .copied();

                                    if let Some(cid) = child_id_opt {
                                        if let Ok(fmeta) = entry.metadata() {
                                            let new_size = scanner::winfs::round_up_to_cluster(
                                                fmeta.len(),
                                                cluster_size,
                                            );
                                            let old_size = tree.nodes[cid.0 as usize].size;
                                            let new_mod = fmeta.modified().ok();
                                            let old_mod = tree.nodes[cid.0 as usize].modified;

                                            if new_size != old_size || new_mod != old_mod {
                                                if new_size != old_size {
                                                    tree.update_node_size(cid, new_size, 1);
                                                    report.bytes_delta +=
                                                        new_size as i64 - old_size as i64;
                                                }
                                                tree.nodes[cid.0 as usize].modified = new_mod;
                                                report.nodes_updated += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        tree.nodes[i].modified = disk_mod;
                    }
                }
            }
        }
    }

    // 3. Synchronize modified records back to SQLite
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE volumes SET last_usn = ?1, last_scan_time = ?2, total_bytes = ?3 WHERE id = ?4",
        params![
            new_high_usn,
            now_secs,
            tree.node(tree.root()).map(|n| n.size).unwrap_or(0) as i64,
            volume_id
        ],
    )?;

    // Update modified nodes in database
    {
        let mut update_stmt = tx.prepare_cached(
            "UPDATE nodes SET size = ?1, file_count = ?2 WHERE volume_id = ?3 AND id = ?4",
        )?;
        for node in &tree.nodes {
            update_stmt.execute(params![
                node.size as i64,
                node.file_count as i64,
                volume_id,
                node.id.0 as i64
            ])?;
        }
    }

    tx.commit()?;
    report.sync_duration_ms = start_time.elapsed().as_millis() as u64;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{DirMetadata, FileMetadata, ScanEvent};
    use std::ffi::OsString;
    use tempfile::tempdir;

    fn dir_meta(name: &str) -> DirMetadata {
        DirMetadata {
            name: OsString::from(name),
            is_reparse_point: false,
        }
    }

    fn file_meta(name: &str, size: u64) -> FileMetadata {
        FileMetadata {
            name: OsString::from(name),
            size,
            extension: Some("txt".to_string()),
            modified: Some(SystemTime::now()),
            created: Some(SystemTime::now()),
            attributes: FileAttributeFlags::default(),
        }
    }

    fn build_test_tree() -> StorageTree {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta(r"C:\TestDrive"),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("sub"),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(1),
                meta: file_meta("hello.txt", 4096),
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(1),
                total_size: 4096,
                file_count: 1,
                dir_count: 0,
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 4096,
                file_count: 1,
                dir_count: 1,
            },
        ];
        StorageTree::build(events).unwrap()
    }

    #[test]
    fn save_and_load_tree_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_index.db");
        let tree = build_test_tree();
        let root = Path::new(r"C:\TestDrive");

        save_tree_to_db(&db_path, &tree, root).expect("save must succeed");

        let loaded_opt = load_tree_from_db(&db_path, root).expect("load must succeed");
        assert!(loaded_opt.is_some(), "tree must be loaded");

        let loaded = loaded_opt.unwrap();
        assert_eq!(loaded.node_count(), tree.node_count());
        assert_eq!(loaded.node(loaded.root()).unwrap().size, 4096);
        assert_eq!(loaded.node(loaded.root()).unwrap().file_count, 1);
    }

    #[test]
    fn get_index_stats_reports_correct_metrics() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_stats.db");
        let tree = build_test_tree();
        let root = Path::new(r"C:\TestDrive");

        save_tree_to_db(&db_path, &tree, root).unwrap();

        let stats = get_index_stats(&db_path, root)
            .unwrap()
            .expect("stats must exist");
        assert_eq!(stats.node_count, 3); // root + sub + hello.txt
        assert_eq!(stats.total_size, 4096);
        assert!(stats.last_scan_time > 0);
    }

    #[test]
    fn delete_indexed_volume_removes_data() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_del.db");
        let tree = build_test_tree();
        let root = Path::new(r"C:\TestDrive");

        save_tree_to_db(&db_path, &tree, root).unwrap();
        let deleted = delete_indexed_volume(&db_path, root).unwrap();
        assert!(deleted);

        let stats = get_index_stats(&db_path, root).unwrap();
        assert!(stats.is_none());
    }

    #[test]
    fn incremental_sync_updates_tree_and_db() {
        let dir = tempdir().unwrap();
        let test_root = dir.path().join("workspace");
        std::fs::create_dir_all(&test_root).unwrap();
        let file_path = test_root.join("data.txt");
        let cluster_size = scanner::winfs::cluster_size(&test_root).unwrap_or(4096);
        std::fs::write(&file_path, vec![0u8; cluster_size as usize]).unwrap();

        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta(test_root.to_str().unwrap()),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("data.txt", cluster_size),
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: cluster_size,
                file_count: 1,
                dir_count: 0,
            },
        ];
        let mut tree = StorageTree::build(events).unwrap();
        let db_path = dir.path().join("index.db");

        save_tree_to_db(&db_path, &tree, &test_root).unwrap();

        // Simulate file modification on disk crossing cluster boundary
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file_path, vec![0u8; (cluster_size * 2) as usize]).unwrap();

        let report = sync_tree_incremental(&db_path, &mut tree, &test_root).unwrap();
        assert_eq!(report.nodes_updated, 1);
        assert_eq!(report.bytes_delta, cluster_size as i64);

        // Verify root node grew in size
        let root_info = tree.node(tree.root()).unwrap();
        assert_eq!(root_info.size, cluster_size * 2);
    }
}
