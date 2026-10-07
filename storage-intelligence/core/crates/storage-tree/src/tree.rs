//! Core tree data structure, build algorithm, and query API.
//!
//! Architecture (ADR-009 #1): arena-backed flat Vec<TreeNode> indexed by NodeId,
//! with a parallel Vec<Vec<NodeId>> for parent→children. Immutable post-build
//! (ADR-009 #2). Children pre-sorted by size descending during build.

use common::{FileAttributeFlags, NodeId, ScanEvent};
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::path::PathBuf;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Node type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
    /// Junction/symlink/mount point leaf (ADR-008 #1).
    ReparsePoint,
    /// Permission-denied / locked directory or file.
    Inaccessible,
}

/// Read-only view of a single node's data. Borrows from the tree — no allocation
/// per query (except for the string slices, which point into the tree's owned data).
#[derive(Debug)]
pub struct NodeInfo<'a> {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub name: &'a str,
    pub size: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub extension: Option<&'a str>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub attributes: Option<FileAttributeFlags>,
}

/// Error during tree construction from scan events.
#[derive(Debug, thiserror::Error)]
pub enum TreeBuildError {
    #[error("empty event stream: no scan events to build a tree from")]
    EmptyEvents,
    #[error("missing root: first event must be EnteredDirectory with parent_id=None")]
    MissingRoot,
}

// ---------------------------------------------------------------------------
// Internal node
// ---------------------------------------------------------------------------

/// Internal node stored in the arena. File-specific fields waste ~40 bytes per
/// directory node — accepted at 1M scale for the simplicity of avoiding an enum
/// with different layouts (see ADR-009 #1 rationale).
#[derive(Debug)]
pub(crate) struct TreeNode {
    pub(crate) id: NodeId,
    pub(crate) parent: Option<NodeId>,
    pub(crate) kind: NodeKind,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) file_count: u64,
    pub(crate) dir_count: u64,
    // File-specific (defaults for directories):
    pub(crate) extension: Option<String>,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) created: Option<SystemTime>,
    pub(crate) attributes: FileAttributeFlags,
}

impl TreeNode {
    fn as_info(&self) -> NodeInfo<'_> {
        NodeInfo {
            id: self.id,
            parent: self.parent,
            kind: self.kind,
            name: &self.name,
            size: self.size,
            file_count: self.file_count,
            dir_count: self.dir_count,
            extension: self.extension.as_deref(),
            modified: self.modified,
            created: self.created,
            attributes: if self.kind == NodeKind::File {
                Some(self.attributes)
            } else {
                None
            },
        }
    }
}

// ---------------------------------------------------------------------------
// StorageTree
// ---------------------------------------------------------------------------

/// The immutable, queryable tree built from a completed scan's events.
///
/// Thread-safe for concurrent reads (`&StorageTree` is `Send + Sync` automatically
/// because all fields are owned, non-mutable data).
pub struct StorageTree {
    /// Arena: nodes[i] has `id == NodeId(tree_id_for_index_i)`.
    /// The mapping is NOT 1:1 with scanner NodeIds because the tree allocates
    /// additional ids for files and inaccessible entries (scanner only ids directories).
    pub(crate) nodes: Vec<TreeNode>,
    /// children[i] = sorted child NodeIds of the node whose tree-local index is i.
    /// For file/inaccessible nodes, this is an empty Vec.
    pub(crate) children: Vec<Vec<NodeId>>,
    /// Root node's index into `nodes`.
    pub(crate) root: usize,
}

impl StorageTree {
    /// Build from a completed scan's event stream. O(n) time, O(n) space.
    ///
    /// Events must be in the order produced by `scanner::scan()`:
    /// - First event must be `EnteredDirectory` with `parent_id = None` (the root).
    /// - `DirectoryComplete` events provide the aggregated size/counts for directories.
    pub fn build(events: Vec<ScanEvent>) -> Result<StorageTree, TreeBuildError> {
        if events.is_empty() {
            return Err(TreeBuildError::EmptyEvents);
        }

        // Pre-size: estimate roughly 1 node per event (directories produce 2 events
        // each — EnteredDirectory + DirectoryComplete — but files produce 1, so
        // events.len() is a reasonable upper-bound estimate).
        let mut nodes: Vec<TreeNode> = Vec::with_capacity(events.len());
        let mut children: Vec<Vec<NodeId>> = Vec::with_capacity(events.len());

        // Map scanner NodeId.0 → index in our nodes Vec.
        // Scanner NodeIds are dense 0..n, so a Vec works as a perfect-hash map.
        // We'll grow this as we encounter directory IDs.
        let mut scanner_id_to_index: Vec<usize> = Vec::new();

        let mut root_index: Option<usize> = None;
        // Tree-internal node id counter (for files/inaccessible entries that
        // don't have scanner-assigned NodeIds).
        let mut next_tree_id: u64 = 0;

        // Helper: find the max scanner NodeId to pre-size the lookup table.
        let max_scanner_id = events.iter().filter_map(|e| match e {
            ScanEvent::EnteredDirectory { id, .. } => Some(id.0),
            ScanEvent::DirectoryComplete { id, .. } => Some(id.0),
            _ => None,
        }).max().unwrap_or(0);
        scanner_id_to_index.resize(max_scanner_id as usize + 1, usize::MAX);

        for event in &events {
            match event {
                ScanEvent::EnteredDirectory { parent_id, id, meta } => {
                    let tree_index = nodes.len();
                    let kind = if meta.is_reparse_point {
                        NodeKind::ReparsePoint
                    } else {
                        NodeKind::Directory
                    };

                    let parent_tree_id = parent_id.map(|pid| {
                        let idx = scanner_id_to_index[pid.0 as usize];
                        nodes[idx].id
                    });

                    let node_id = NodeId(next_tree_id);
                    next_tree_id += 1;

                    nodes.push(TreeNode {
                        id: node_id,
                        parent: parent_tree_id,
                        kind,
                        name: meta.name.to_string_lossy().into_owned(),
                        size: 0,
                        file_count: 0,
                        dir_count: 0,
                        extension: None,
                        modified: None,
                        created: None,
                        attributes: FileAttributeFlags::default(),
                    });
                    children.push(Vec::new());

                    // Map scanner id → tree index.
                    scanner_id_to_index[id.0 as usize] = tree_index;

                    if parent_id.is_none() {
                        root_index = Some(tree_index);
                    } else if let Some(pid) = parent_id {
                        let parent_idx = scanner_id_to_index[pid.0 as usize];
                        children[parent_idx].push(node_id);
                    }
                }

                ScanEvent::FileFound { parent_id, meta } => {
                    let _tree_index = nodes.len();
                    let node_id = NodeId(next_tree_id);
                    next_tree_id += 1;

                    let parent_tree_id = {
                        let idx = scanner_id_to_index[parent_id.0 as usize];
                        nodes[idx].id
                    };

                    nodes.push(TreeNode {
                        id: node_id,
                        parent: Some(parent_tree_id),
                        kind: NodeKind::File,
                        name: meta.name.to_string_lossy().into_owned(),
                        size: meta.size,
                        file_count: 0,
                        dir_count: 0,
                        extension: meta.extension.clone(),
                        modified: meta.modified,
                        created: meta.created,
                        attributes: meta.attributes,
                    });
                    children.push(Vec::new());

                    let parent_idx = scanner_id_to_index[parent_id.0 as usize];
                    children[parent_idx].push(node_id);
                }

                ScanEvent::Inaccessible { parent_id, path, reason: _ } => {
                    let _tree_index = nodes.len();
                    let node_id = NodeId(next_tree_id);
                    next_tree_id += 1;

                    let parent_tree_id = {
                        let idx = scanner_id_to_index[parent_id.0 as usize];
                        nodes[idx].id
                    };

                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());

                    nodes.push(TreeNode {
                        id: node_id,
                        parent: Some(parent_tree_id),
                        kind: NodeKind::Inaccessible,
                        name,
                        size: 0,
                        file_count: 0,
                        dir_count: 0,
                        extension: None,
                        modified: None,
                        created: None,
                        attributes: FileAttributeFlags::default(),
                    });
                    children.push(Vec::new());

                    let parent_idx = scanner_id_to_index[parent_id.0 as usize];
                    children[parent_idx].push(node_id);
                }

                ScanEvent::DirectoryComplete {
                    id,
                    total_size,
                    file_count,
                    dir_count,
                } => {
                    let idx = scanner_id_to_index[id.0 as usize];
                    nodes[idx].size = *total_size;
                    nodes[idx].file_count = *file_count;
                    nodes[idx].dir_count = *dir_count;
                }
            }
        }

        let root = root_index.ok_or(TreeBuildError::MissingRoot)?;

        // Sort each directory's children by size descending (largest first)
        // so the treemap and "children of node" queries return in display order.
        for child_list in &mut children {
            child_list.sort_unstable_by(|a, b| {
                let size_a = nodes[a.0 as usize].size;
                let size_b = nodes[b.0 as usize].size;
                size_b.cmp(&size_a) // descending
            });
        }

        Ok(StorageTree {
            nodes,
            children,
            root,
        })
    }

    // -----------------------------------------------------------------------
    // Query API
    // -----------------------------------------------------------------------

    /// Root node id.
    pub fn root(&self) -> NodeId {
        self.nodes[self.root].id
    }

    /// Total number of nodes in the tree.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Lookup a single node by its tree-local NodeId. O(1).
    pub fn node(&self, id: NodeId) -> Option<NodeInfo<'_>> {
        let idx = id.0 as usize;
        self.nodes.get(idx).map(|n| n.as_info())
    }

    /// Children of a directory node, pre-sorted by size descending.
    /// Returns the full sorted slice. Callers paginate via `[offset..offset+limit]`.
    /// Returns `None` if the id is out of range. Returns an empty slice for
    /// files/inaccessible nodes (they have no children).
    pub fn children(&self, id: NodeId) -> Option<&[NodeId]> {
        let idx = id.0 as usize;
        self.children.get(idx).map(|v| v.as_slice())
    }

    /// Ancestors from node to root (inclusive), bottom-up.
    /// Returns `[node_id, parent_id, ..., root_id]`.
    /// For breadcrumb navigation (PRD §7). O(depth).
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut current = id;
        loop {
            let idx = current.0 as usize;
            if idx >= self.nodes.len() {
                break;
            }
            result.push(current);
            match self.nodes[idx].parent {
                Some(parent_id) => current = parent_id,
                None => break,
            }
        }
        result
    }

    /// Reconstruct the full path of a node. O(depth).
    /// Walks parent pointers to root, collects names, reverses.
    pub fn full_path(&self, id: NodeId) -> Option<PathBuf> {
        let idx = id.0 as usize;
        if idx >= self.nodes.len() {
            return None;
        }

        let ancestors = self.ancestors(id);
        let mut path = PathBuf::new();
        for &ancestor_id in ancestors.iter().rev() {
            let aidx = ancestor_id.0 as usize;
            path.push(&self.nodes[aidx].name);
        }
        Some(path)
    }

    /// Lookup a node by absolute filesystem path. O(depth * branching_factor).
    /// Returns `Some(NodeId)` if the path exists within this tree, or `None`.
    pub fn find_by_path(&self, target: &std::path::Path) -> Option<NodeId> {
        let root_path = self.full_path(self.root())?;
        let root_norm = root_path.to_string_lossy().replace('/', "\\");
        let target_norm = target.to_string_lossy().replace('/', "\\");

        let root_clean = root_norm.trim_end_matches('\\');
        let target_clean = target_norm.trim_end_matches('\\');

        if target_clean.eq_ignore_ascii_case(root_clean) {
            return Some(self.root());
        }

        let prefix = format!("{}\\", root_clean);
        if !target_clean.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()) {
            return None;
        }

        let relative = &target_clean[prefix.len()..];
        let components: Vec<&str> = relative.split('\\').filter(|s| !s.is_empty()).collect();

        let mut current = self.root();
        for comp in components {
            let children = self.children(current)?;
            let mut found = None;
            for &child_id in children {
                let child_idx = child_id.0 as usize;
                if self.nodes[child_idx].name.eq_ignore_ascii_case(comp) {
                    found = Some(child_id);
                    break;
                }
            }
            match found {
                Some(next_id) => current = next_id,
                None => return None,
            }
        }
        Some(current)
    }

    /// Top N largest files anywhere under `root_id` (recursive).
    /// Uses a min-heap of size `n` for efficient top-N selection. O(total_nodes_under_root).
    pub fn top_files_by_size(&self, root_id: NodeId, n: usize) -> Vec<NodeId> {
        if n == 0 {
            return Vec::new();
        }

        // Min-heap keyed by file size, capped at n entries.
        let mut heap: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(n + 1);

        self.collect_top_files(root_id, n, &mut heap);

        // Extract results in descending order.
        let mut result: Vec<NodeId> = heap.into_iter().map(|Reverse((_, id))| id).collect();
        result.sort_unstable_by(|a, b| {
            let size_a = self.nodes[a.0 as usize].size;
            let size_b = self.nodes[b.0 as usize].size;
            size_b.cmp(&size_a)
        });
        result
    }

    fn collect_top_files(
        &self,
        node_id: NodeId,
        n: usize,
        heap: &mut BinaryHeap<Reverse<(u64, NodeId)>>,
    ) {
        let idx = node_id.0 as usize;
        if idx >= self.nodes.len() {
            return;
        }
        let node = &self.nodes[idx];

        if node.kind == NodeKind::File {
            heap.push(Reverse((node.size, node.id)));
            if heap.len() > n {
                heap.pop(); // remove smallest
            }
            return;
        }

        // Recurse into children for directory/reparse-point nodes.
        if let Some(child_ids) = self.children.get(idx) {
            for &child_id in child_ids {
                self.collect_top_files(child_id, n, heap);
            }
        }
    }

    /// Computes a squarified treemap layout (ADR-010) for the subtree rooted at `root_id`.
    pub fn compute_layout(
        &self,
        root_id: NodeId,
        width: f32,
        height: f32,
        max_depth: u32,
        min_size_px: f32,
    ) -> Vec<crate::treemap::TreemapRect> {
        crate::treemap::compute_treemap_layout(self, root_id, width, height, max_depth, min_size_px)
    }

    /// Reconstructs a StorageTree from raw flat arena nodes (ADR-016).
    /// Used by the persistence engine to hydrate trees from SQLite in < 100ms.
    pub(crate) fn reconstruct(nodes: Vec<TreeNode>, root: usize) -> StorageTree {
        let mut children: Vec<Vec<NodeId>> = vec![Vec::new(); nodes.len()];
        for (i, node) in nodes.iter().enumerate() {
            if let Some(pid) = node.parent {
                let p_idx = pid.0 as usize;
                if p_idx < children.len() {
                    children[p_idx].push(NodeId(i as u64));
                }
            }
        }
        for child_list in &mut children {
            child_list.sort_unstable_by(|a, b| {
                let size_a = nodes[a.0 as usize].size;
                let size_b = nodes[b.0 as usize].size;
                size_b.cmp(&size_a)
            });
        }
        StorageTree {
            nodes,
            children,
            root,
        }
    }

    /// Applies a delta in size and file count to a node and propagates the delta up to the root (ADR-016).
    pub fn update_node_size(&mut self, node_id: NodeId, new_size: u64, new_file_count: u64) {
        let idx = node_id.0 as usize;
        if idx >= self.nodes.len() {
            return;
        }
        let old_size = self.nodes[idx].size;
        let old_file_count = self.nodes[idx].file_count;

        self.nodes[idx].size = new_size;
        self.nodes[idx].file_count = new_file_count;

        let delta_size = new_size as i64 - old_size as i64;
        let delta_files = new_file_count as i64 - old_file_count as i64;

        if delta_size == 0 && delta_files == 0 {
            return;
        }

        let mut curr_parent = self.nodes[idx].parent;
        while let Some(pid) = curr_parent {
            let p_idx = pid.0 as usize;
            if p_idx >= self.nodes.len() {
                break;
            }
            if delta_size > 0 {
                self.nodes[p_idx].size = self.nodes[p_idx].size.saturating_add(delta_size as u64);
            } else if delta_size < 0 {
                self.nodes[p_idx].size = self.nodes[p_idx].size.saturating_sub((-delta_size) as u64);
            }

            if delta_files > 0 {
                self.nodes[p_idx].file_count = self.nodes[p_idx].file_count.saturating_add(delta_files as u64);
            } else if delta_files < 0 {
                self.nodes[p_idx].file_count = self.nodes[p_idx].file_count.saturating_sub((-delta_files) as u64);
            }

            let children_vec = &mut self.children[p_idx];
            children_vec.sort_unstable_by(|a, b| {
                let size_a = self.nodes[a.0 as usize].size;
                let size_b = self.nodes[b.0 as usize].size;
                size_b.cmp(&size_a)
            });

            curr_parent = self.nodes[p_idx].parent;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use common::{DirMetadata, FileMetadata, FileAttributeFlags, NodeId, ScanEvent};
    use std::ffi::OsString;

    fn dir_meta(name: &str, is_reparse: bool) -> DirMetadata {
        DirMetadata {
            name: OsString::from(name),
            is_reparse_point: is_reparse,
        }
    }

    fn file_meta(name: &str, size: u64, ext: Option<&str>) -> FileMetadata {
        FileMetadata {
            name: OsString::from(name),
            size,
            extension: ext.map(|s| s.to_string()),
            modified: None,
            created: None,
            attributes: FileAttributeFlags::default(),
        }
    }

    #[test]
    fn empty_events_returns_error() {
        let result = StorageTree::build(vec![]);
        assert!(matches!(result, Err(TreeBuildError::EmptyEvents)));
    }

    #[test]
    fn single_root_directory() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("C:", false),
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 0,
                file_count: 0,
                dir_count: 0,
            },
        ];

        let tree = StorageTree::build(events).unwrap();
        assert_eq!(tree.node_count(), 1);
        let root = tree.node(tree.root()).unwrap();
        assert_eq!(root.name, "C:");
        assert_eq!(root.kind, NodeKind::Directory);
        assert_eq!(root.size, 0);
        assert!(tree.children(tree.root()).unwrap().is_empty());
    }

    #[test]
    fn flat_directory_with_files() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("big.zip", 1000, Some("zip")),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("small.txt", 100, Some("txt")),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("medium.doc", 500, Some("doc")),
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 1600,
                file_count: 3,
                dir_count: 0,
            },
        ];

        let tree = StorageTree::build(events).unwrap();
        assert_eq!(tree.node_count(), 4); // root + 3 files

        let root_info = tree.node(tree.root()).unwrap();
        assert_eq!(root_info.size, 1600);
        assert_eq!(root_info.file_count, 3);

        let child_ids = tree.children(tree.root()).unwrap();
        assert_eq!(child_ids.len(), 3);

        // Children sorted by size descending.
        let sizes: Vec<u64> = child_ids
            .iter()
            .map(|id| tree.node(*id).unwrap().size)
            .collect();
        assert_eq!(sizes, vec![1000, 500, 100]);
    }

    #[test]
    fn nested_tree_with_correct_aggregation() {
        // root/
        //   sub1/
        //     deep.txt (300 bytes)
        //   file1.txt (200 bytes)
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("sub1", false),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(1),
                meta: file_meta("deep.txt", 300, Some("txt")),
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(1),
                total_size: 300,
                file_count: 1,
                dir_count: 0,
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("file1.txt", 200, Some("txt")),
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 500,
                file_count: 2,
                dir_count: 1,
            },
        ];

        let tree = StorageTree::build(events).unwrap();
        assert_eq!(tree.node_count(), 4); // root, sub1, deep.txt, file1.txt

        let root_info = tree.node(tree.root()).unwrap();
        assert_eq!(root_info.size, 500);
        assert_eq!(root_info.file_count, 2);
        assert_eq!(root_info.dir_count, 1);

        // sub1 should be the larger child (300 > 200), so first in sorted order.
        let root_children = tree.children(tree.root()).unwrap();
        assert_eq!(root_children.len(), 2);
        let first_child = tree.node(root_children[0]).unwrap();
        assert_eq!(first_child.name, "sub1");
        assert_eq!(first_child.size, 300);
        assert_eq!(first_child.kind, NodeKind::Directory);
    }

    #[test]
    fn ancestors_returns_bottom_up_path() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("level1", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(1)),
                id: NodeId(2),
                meta: dir_meta("level2", false),
            },
            ScanEvent::DirectoryComplete { id: NodeId(2), total_size: 0, file_count: 0, dir_count: 0 },
            ScanEvent::DirectoryComplete { id: NodeId(1), total_size: 0, file_count: 0, dir_count: 1 },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 0, file_count: 0, dir_count: 2 },
        ];

        let tree = StorageTree::build(events).unwrap();

        // level2's tree id is NodeId(2)
        let ancestors = tree.ancestors(NodeId(2));
        assert_eq!(ancestors.len(), 3);
        // Bottom-up: level2, level1, root
        let names: Vec<&str> = ancestors.iter().map(|id| tree.node(*id).unwrap().name).collect();
        assert_eq!(names, vec!["level2", "level1", "root"]);
    }

    #[test]
    fn full_path_reconstruction() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("C:", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("Users", false),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(1),
                meta: file_meta("readme.md", 50, Some("md")),
            },
            ScanEvent::DirectoryComplete { id: NodeId(1), total_size: 50, file_count: 1, dir_count: 0 },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 50, file_count: 1, dir_count: 1 },
        ];

        let tree = StorageTree::build(events).unwrap();

        // File readme.md has tree NodeId(2) (dirs are 0, 1; file is 2)
        let path = tree.full_path(NodeId(2)).unwrap();
        assert_eq!(path, PathBuf::from("C:").join("Users").join("readme.md"));
    }

    #[test]
    fn top_files_by_size_returns_correct_top_n() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("a.txt", 100, None) },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("b.txt", 500, None) },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("c.txt", 300, None) },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("d.txt", 50, None) },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("e.txt", 800, None) },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 1750,
                file_count: 5,
                dir_count: 0,
            },
        ];

        let tree = StorageTree::build(events).unwrap();

        let top3 = tree.top_files_by_size(tree.root(), 3);
        assert_eq!(top3.len(), 3);
        let sizes: Vec<u64> = top3.iter().map(|id| tree.node(*id).unwrap().size).collect();
        assert_eq!(sizes, vec![800, 500, 300]);
    }

    #[test]
    fn top_files_by_size_in_nested_tree() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("top.txt", 100, None) },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("sub", false),
            },
            ScanEvent::FileFound { parent_id: NodeId(1), meta: file_meta("deep.txt", 900, None) },
            ScanEvent::DirectoryComplete { id: NodeId(1), total_size: 900, file_count: 1, dir_count: 0 },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 1000, file_count: 2, dir_count: 1 },
        ];

        let tree = StorageTree::build(events).unwrap();

        let top1 = tree.top_files_by_size(tree.root(), 1);
        assert_eq!(top1.len(), 1);
        assert_eq!(tree.node(top1[0]).unwrap().name, "deep.txt");
        assert_eq!(tree.node(top1[0]).unwrap().size, 900);
    }

    #[test]
    fn reparse_point_is_leaf_with_no_children() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("junction_link", true),
            },
            ScanEvent::DirectoryComplete { id: NodeId(1), total_size: 0, file_count: 0, dir_count: 0 },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 0, file_count: 0, dir_count: 1 },
        ];

        let tree = StorageTree::build(events).unwrap();

        let junction = tree.node(NodeId(1)).unwrap();
        assert_eq!(junction.kind, NodeKind::ReparsePoint);
        assert_eq!(junction.name, "junction_link");
        assert!(tree.children(NodeId(1)).unwrap().is_empty());
    }

    #[test]
    fn inaccessible_node_is_recorded() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::Inaccessible {
                parent_id: NodeId(0),
                path: PathBuf::from("root\\blocked"),
                reason: common::InaccessibleReason::PermissionDenied,
            },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 0, file_count: 0, dir_count: 0 },
        ];

        let tree = StorageTree::build(events).unwrap();
        assert_eq!(tree.node_count(), 2); // root + inaccessible

        let children = tree.children(tree.root()).unwrap();
        assert_eq!(children.len(), 1);
        let blocked = tree.node(children[0]).unwrap();
        assert_eq!(blocked.kind, NodeKind::Inaccessible);
        assert_eq!(blocked.name, "blocked");
    }

    #[test]
    fn node_lookup_out_of_range_returns_none() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 0, file_count: 0, dir_count: 0 },
        ];

        let tree = StorageTree::build(events).unwrap();
        assert!(tree.node(NodeId(999)).is_none());
    }

    #[test]
    fn children_of_file_returns_empty_slice() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("file.txt", 100, None),
            },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 100, file_count: 1, dir_count: 0 },
        ];

        let tree = StorageTree::build(events).unwrap();
        // File's tree NodeId is 1
        let children = tree.children(NodeId(1)).unwrap();
        assert!(children.is_empty());
    }

    #[test]
    fn children_sorted_by_size_descending() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("tiny.txt", 10, None) },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("huge.bin", 9999, None) },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("mid.doc", 500, None) },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 10509,
                file_count: 3,
                dir_count: 0,
            },
        ];

        let tree = StorageTree::build(events).unwrap();
        let children = tree.children(tree.root()).unwrap();
        let names: Vec<&str> = children.iter().map(|id| tree.node(*id).unwrap().name).collect();
        assert_eq!(names, vec!["huge.bin", "mid.doc", "tiny.txt"]);
    }

    #[test]
    fn large_synthetic_build_completes_quickly() {
        // 100K events: ~33K directories (enter+complete = 66K events) + ~34K files
        let mut events = Vec::with_capacity(100_000);
        let mut dir_id: u64 = 0;

        // Root
        events.push(ScanEvent::EnteredDirectory {
            parent_id: None,
            id: NodeId(dir_id),
            meta: dir_meta("root", false),
        });

        // Create 100 subdirectories, each with 333 files
        for d in 1..=100u64 {
            dir_id += 1;
            events.push(ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(dir_id),
                meta: dir_meta(&format!("dir{d}"), false),
            });

            for f in 0..333 {
                events.push(ScanEvent::FileFound {
                    parent_id: NodeId(dir_id),
                    meta: file_meta(&format!("file{f}.bin"), (f + 1) * 1024, Some("bin")),
                });
            }

            events.push(ScanEvent::DirectoryComplete {
                id: NodeId(dir_id),
                total_size: (1..=333u64).map(|f| f * 1024).sum(),
                file_count: 333,
                dir_count: 0,
            });
        }

        // Root complete
        let total_size: u64 = (1..=333u64).map(|f| f * 1024).sum::<u64>() * 100;
        events.push(ScanEvent::DirectoryComplete {
            id: NodeId(0),
            total_size,
            file_count: 33300,
            dir_count: 100,
        });

        let start = std::time::Instant::now();
        let tree = StorageTree::build(events).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(tree.node_count(), 1 + 100 + 33300); // root + 100 dirs + 33300 files
        assert!(elapsed.as_secs() < 1, "build took {:?}, expected < 1s", elapsed);
    }

    #[test]
    fn top_files_with_n_zero_returns_empty() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound { parent_id: NodeId(0), meta: file_meta("f.txt", 100, None) },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 100, file_count: 1, dir_count: 0 },
        ];

        let tree = StorageTree::build(events).unwrap();
        assert!(tree.top_files_by_size(tree.root(), 0).is_empty());
    }

    #[test]
    fn file_extension_preserved() {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta("root", false),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: file_meta("photo.jpg", 5000, Some("jpg")),
            },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 5000, file_count: 1, dir_count: 0 },
        ];

        let tree = StorageTree::build(events).unwrap();
        let file = tree.node(NodeId(1)).unwrap();
        assert_eq!(file.extension, Some("jpg"));
        assert_eq!(file.kind, NodeKind::File);
    }

    #[test]
    fn find_by_path_matches_nested_nodes() {
        use std::path::Path;
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: dir_meta(r"C:\TestRoot", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: dir_meta("Program Files", false),
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(1)),
                id: NodeId(2),
                meta: dir_meta("AppVendor", false),
            },
            ScanEvent::FileFound {
                parent_id: NodeId(2),
                meta: file_meta("app.exe", 1048576, Some("exe")),
            },
            ScanEvent::DirectoryComplete { id: NodeId(2), total_size: 1048576, file_count: 1, dir_count: 0 },
            ScanEvent::DirectoryComplete { id: NodeId(1), total_size: 1048576, file_count: 1, dir_count: 1 },
            ScanEvent::DirectoryComplete { id: NodeId(0), total_size: 1048576, file_count: 1, dir_count: 2 },
        ];

        let tree = StorageTree::build(events).unwrap();

        // Match root
        let root_match = tree.find_by_path(Path::new(r"C:\TestRoot"));
        assert_eq!(root_match, Some(tree.root()));

        // Match nested directory
        let vendor_match = tree.find_by_path(Path::new(r"C:\TestRoot\Program Files\AppVendor"));
        assert!(vendor_match.is_some());
        let vendor_info = tree.node(vendor_match.unwrap()).unwrap();
        assert_eq!(vendor_info.name, "AppVendor");
        assert_eq!(vendor_info.size, 1048576);

        // Case-insensitivity match
        let case_match = tree.find_by_path(Path::new(r"c:\testroot\program files\appvendor\app.exe"));
        assert!(case_match.is_some());
        let app_info = tree.node(case_match.unwrap()).unwrap();
        assert_eq!(app_info.name, "app.exe");

        // Non-existent path returns None
        let missing = tree.find_by_path(Path::new(r"C:\TestRoot\Program Files\NotFound"));
        assert_eq!(missing, None);
    }
}
