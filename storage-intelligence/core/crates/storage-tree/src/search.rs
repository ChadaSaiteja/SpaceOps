//! Search and indexing engine for StorageTree (ADR-012).
//!
//! Provides sub-5ms in-memory queries with rich token filtering (name, extension, size,
//! category, kind), space-aware relevance ranking, and bounded top-K result limits.

use common::NodeId;
use crate::tree::{NodeKind, StorageTree};
use crate::treemap::{classify_extension, FileCategory};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::PathBuf;

/// Structured query model parsed from user search input.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub extension: Option<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub category: Option<FileCategory>,
    pub kind: Option<NodeKind>,
}

impl SearchQuery {
    /// Parse a query string into a structured `SearchQuery`.
    ///
    /// Supports:
    /// - `ext:<ext>` (e.g. `ext:vhdx`, `ext:iso`, `ext:log`)
    /// - `size:>1GB`, `size:<10MB`, `size:100MB..500MB`, `>1GB`, `<500MB`
    /// - `type:<cat>` (e.g. `type:video`, `type:audio`, `type:doc`, `type:archive`, `type:code`)
    /// - `kind:<file|dir>` (e.g. `kind:file`, `kind:dir`, `kind:folder`)
    /// - Free-text terms (e.g. `docker`, `appdata`, `windows`)
    pub fn parse(input: &str) -> Self {
        let mut query = SearchQuery::default();
        let mut text_tokens = Vec::new();

        for token in input.split_whitespace() {
            let lower = token.to_ascii_lowercase();

            if let Some(ext) = lower.strip_prefix("ext:") {
                let trimmed = ext.trim_start_matches('.');
                if !trimmed.is_empty() {
                    query.extension = Some(trimmed.to_string());
                }
            } else if let Some(size_spec) = lower.strip_prefix("size:") {
                parse_size_filter(size_spec, &mut query);
            } else if let Some(cat_str) = lower.strip_prefix("type:").or_else(|| lower.strip_prefix("cat:")) {
                query.category = parse_category(cat_str);
            } else if let Some(kind_str) = lower.strip_prefix("kind:") {
                query.kind = match kind_str {
                    "file" | "f" => Some(NodeKind::File),
                    "dir" | "directory" | "folder" | "d" => Some(NodeKind::Directory),
                    _ => None,
                };
            } else if lower.starts_with(">") || lower.starts_with("<") {
                parse_size_filter(&lower, &mut query);
            } else if lower.starts_with('.') && lower.len() > 1 && !lower.contains('\\') && !lower.contains('/') {
                query.extension = Some(lower[1..].to_string());
            } else {
                text_tokens.push(lower);
            }
        }

        if !text_tokens.is_empty() {
            query.text = Some(text_tokens.join(" "));
        }

        query
    }
}

fn parse_size_filter(spec: &str, query: &mut SearchQuery) {
    if let Some((min_str, max_str)) = spec.split_once("..") {
        query.min_size = parse_bytes(min_str);
        query.max_size = parse_bytes(max_str);
    } else if let Some(rest) = spec.strip_prefix(">=") {
        query.min_size = parse_bytes(rest);
    } else if let Some(rest) = spec.strip_prefix('>') {
        query.min_size = parse_bytes(rest);
    } else if let Some(rest) = spec.strip_prefix("<=") {
        query.max_size = parse_bytes(rest);
    } else if let Some(rest) = spec.strip_prefix('<') {
        query.max_size = parse_bytes(rest);
    } else {
        query.min_size = parse_bytes(spec);
    }
}

fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_part, unit_part) = if let Some(idx) = s.find(|c: char| c.is_alphabetic()) {
        (&s[..idx], &s[idx..])
    } else {
        (s, "")
    };

    let val: f64 = num_part.parse().ok()?;
    let multiplier: f64 = match unit_part.to_ascii_lowercase().as_str() {
        "tb" | "t" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "gb" | "g" => 1024.0 * 1024.0 * 1024.0,
        "mb" | "m" => 1024.0 * 1024.0,
        "kb" | "k" => 1024.0,
        "b" | "" => 1.0,
        _ => return None,
    };

    Some((val * multiplier) as u64)
}

fn parse_category(s: &str) -> Option<FileCategory> {
    match s {
        "video" | "vid" | "movie" => Some(FileCategory::Video),
        "audio" | "sound" | "music" => Some(FileCategory::Audio),
        "image" | "img" | "photo" | "picture" => Some(FileCategory::Image),
        "doc" | "document" | "text" => Some(FileCategory::Document),
        "archive" | "zip" | "compressed" => Some(FileCategory::Archive),
        "exe" | "executable" | "program" | "app" => Some(FileCategory::Executable),
        "code" | "dev" | "developer" => Some(FileCategory::Code),
        "system" | "sys" => Some(FileCategory::System),
        "other" => Some(FileCategory::Other),
        _ => None,
    }
}

/// A ranked search hit.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_id: NodeId,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub kind: NodeKind,
    pub category: FileCategory,
    pub score: u32,
}

impl StorageTree {
    /// Execute a search across the in-memory StorageTree arena.
    ///
    /// Evaluates filtering predicates in a single linear cache pass and bounds output
    /// to the top `max_results` using an in-place min-heap. Full filesystem path
    /// reconstruction is deferred and performed ONLY on the final ranked hits.
    pub fn search(&self, query: &SearchQuery, max_results: usize) -> Vec<SearchResult> {
        if max_results == 0 || self.nodes.is_empty() {
            return Vec::new();
        }

        // Min-heap tracking top K results: (score, size, node_index)
        let mut heap: BinaryHeap<Reverse<(u32, u64, usize)>> = BinaryHeap::with_capacity(max_results + 1);

        for (idx, node) in self.nodes.iter().enumerate() {
            // 1. Kind filter
            if let Some(kind) = query.kind {
                if node.kind != kind {
                    continue;
                }
            }

            // 2. Size filters
            if let Some(min) = query.min_size {
                if node.size < min {
                    continue;
                }
            }
            if let Some(max) = query.max_size {
                if node.size > max {
                    continue;
                }
            }

            // 3. Extension filter
            if let Some(ref ext_filter) = query.extension {
                if node.kind != NodeKind::File {
                    continue;
                }
                match node.extension.as_deref() {
                    Some(e) if e.eq_ignore_ascii_case(ext_filter) => {}
                    _ => continue,
                }
            }

            // 4. Category filter
            let category = classify_extension(node.extension.as_deref());
            if let Some(cat_filter) = query.category {
                if category != cat_filter {
                    continue;
                }
            }

            // 5. Text filter
            let mut match_score = 100u32;
            if let Some(ref needle) = query.text {
                let lower_name = node.name.to_ascii_lowercase();
                if lower_name == *needle {
                    match_score = 1000;
                } else if lower_name.starts_with(needle) {
                    match_score = 500;
                } else if lower_name.contains(needle) {
                    match_score = 100;
                } else {
                    continue;
                }
            }

            // ADR-012: Space-aware size weighting (larger files score higher within same match tier)
            let size_boost = if node.size > 0 {
                // Log2 scale boost capped at 300 points
                (node.size.ilog2() * 5).min(300) as u32
            } else {
                0
            };
            let total_score = match_score + size_boost;

            let entry = Reverse((total_score, node.size, idx));
            if heap.len() < max_results {
                heap.push(entry);
            } else if let Some(&Reverse(smallest)) = heap.peek() {
                if (total_score, node.size, idx) > (smallest.0, smallest.1, smallest.2) {
                    heap.pop();
                    heap.push(entry);
                }
            }
        }

        // Extract and sort results descending
        let mut ranked_indices: Vec<(u32, u64, usize)> = heap.into_iter().map(|Reverse(x)| x).collect();
        ranked_indices.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

        // Reconstruct paths only for the top K results
        let mut results = Vec::with_capacity(ranked_indices.len());
        for (score, _, idx) in ranked_indices {
            let node = &self.nodes[idx];
            let path = self.full_path(node.id).unwrap_or_else(|| PathBuf::from(&node.name));
            let category = classify_extension(node.extension.as_deref());

            results.push(SearchResult {
                node_id: node.id,
                name: node.name.clone(),
                path,
                size: node.size,
                kind: node.kind,
                category,
                score,
            });
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{FileAttributeFlags, NodeId, ScanEvent};
    use crate::tree::StorageTree;

    fn build_test_tree() -> StorageTree {
        let events = vec![
            ScanEvent::EnteredDirectory {
                parent_id: None,
                id: NodeId(0),
                meta: common::DirMetadata {
                    name: "C:\\".into(),
                    is_reparse_point: false,
                },
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: common::FileMetadata {
                    name: "docker_vm.vhdx".into(),
                    size: 15 * 1024 * 1024 * 1024, // 15 GB
                    extension: Some("vhdx".into()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: common::FileMetadata {
                    name: "movie.mp4".into(),
                    size: 2 * 1024 * 1024 * 1024, // 2 GB
                    extension: Some("mp4".into()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::FileFound {
                parent_id: NodeId(0),
                meta: common::FileMetadata {
                    name: "notes.txt".into(),
                    size: 4096,
                    extension: Some("txt".into()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::EnteredDirectory {
                parent_id: Some(NodeId(0)),
                id: NodeId(1),
                meta: common::DirMetadata {
                    name: "node_modules".into(),
                    is_reparse_point: false,
                },
            },
            ScanEvent::FileFound {
                parent_id: NodeId(1),
                meta: common::FileMetadata {
                    name: "package.json".into(),
                    size: 8192,
                    extension: Some("json".into()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(1),
                total_size: 8192,
                file_count: 1,
                dir_count: 0,
            },
            ScanEvent::DirectoryComplete {
                id: NodeId(0),
                total_size: 17 * 1024 * 1024 * 1024 + 12288,
                file_count: 4,
                dir_count: 1,
            },
        ];

        StorageTree::build(events).unwrap()
    }

    #[test]
    fn query_parser_extracts_all_token_types() {
        let q = SearchQuery::parse("docker ext:vhdx size:>1GB type:dev kind:file");
        assert_eq!(q.text, Some("docker".into()));
        assert_eq!(q.extension, Some("vhdx".into()));
        assert_eq!(q.min_size, Some(1024 * 1024 * 1024));
        assert_eq!(q.category, Some(FileCategory::Code));
        assert_eq!(q.kind, Some(NodeKind::File));
    }

    #[test]
    fn search_by_name_substring() {
        let tree = build_test_tree();
        let q = SearchQuery::parse("docker");
        let results = tree.search(&q, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "docker_vm.vhdx");
        assert_eq!(results[0].category, FileCategory::Other);
    }

    #[test]
    fn search_by_extension_filter() {
        let tree = build_test_tree();
        let q = SearchQuery::parse("ext:mp4");
        let results = tree.search(&q, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "movie.mp4");
        assert_eq!(results[0].category, FileCategory::Video);
    }

    #[test]
    fn search_by_size_range() {
        let tree = build_test_tree();
        let q = SearchQuery::parse("size:>1GB kind:file");
        let results = tree.search(&q, 10);

        assert_eq!(results.len(), 2);
        // Ranked by score & size descending: 15GB > 2GB
        assert_eq!(results[0].name, "docker_vm.vhdx");
        assert_eq!(results[1].name, "movie.mp4");
    }

    #[test]
    fn search_by_category_filter() {
        let tree = build_test_tree();
        let q = SearchQuery::parse("type:video");
        let results = tree.search(&q, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "movie.mp4");
    }

    #[test]
    fn search_directories_only() {
        let tree = build_test_tree();
        let q = SearchQuery::parse("node_modules kind:dir");
        let results = tree.search(&q, 10);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "node_modules");
        assert_eq!(results[0].kind, NodeKind::Directory);
    }
}
