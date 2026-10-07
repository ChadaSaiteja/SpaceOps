//! In-memory hierarchical storage tree, built from the scanner's flat `ScanEvent` stream
//! (ADR-008 #4, ADR-009). Provides an efficient, immutable, queryable tree structure
//! that the UI/treemap can consume via FFI queries (ADR-001 §2: batched, on-demand).
//!
//! Design: docs/components/02-storage-tree-design.md
//! Decisions: docs/decisions/ADR-009-storage-tree-key-decisions.md

mod tree;
pub mod treemap;
pub mod search;
pub mod cleanup;
pub mod app_manager;
pub mod developer;
pub mod index;

pub use tree::{NodeInfo, NodeKind, StorageTree, TreeBuildError};
pub use treemap::{classify_extension, compute_treemap_layout, FileCategory, TreemapRect};
pub use search::{SearchQuery, SearchResult};
pub use cleanup::{is_path_protected, CleanupCandidate, CleanupReport, CleanupRuleId, RiskLevel};
pub use app_manager::{AppInfo, AppKind, AppLeftover, LeftoverLocationType, attribute_app_size};
pub use developer::{
    clean_dev_artifact, detect_dev_artifacts, DevArtifact, DevArtifactKind, DevEcosystem,
};
pub use index::{
    delete_indexed_volume, get_index_stats, load_tree_from_db, save_tree_to_db,
    sync_tree_incremental, IndexStats, IndexSyncReport,
};
