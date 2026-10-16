//! Opaque handle types handed to the host across the C ABI (ADR-001/009).

use common::CancellationToken;

/// Opaque handle wrapping a `CancellationToken`, owned by the caller until
/// `cancel_token_destroy`. Safe to call `cancel_token_cancel` from a different
/// thread than the one blocked inside `scan_drive` (ADR-007's threading model).
pub struct CancelHandle(pub(crate) CancellationToken);

/// Opaque handle wrapping a completed scan's summary and event stream, owned by
/// the caller until `scan_result_destroy`. Events are consumed (moved) by
/// `tree_create` to build the storage tree (ADR-009 #5: two-handle lifecycle).
pub struct ScanResultHandle {
    pub(crate) summary: common::ScanSummary,
    /// `Some` until `tree_create` moves the events out; `None` afterward.
    pub(crate) events: Option<Vec<common::ScanEvent>>,
}

/// Opaque handle wrapping a built `StorageTree` (ADR-009). Owned by the caller
/// until `tree_destroy`. Thread-safe for concurrent reads.
pub struct TreeHandle(pub(crate) storage_tree::StorageTree);
