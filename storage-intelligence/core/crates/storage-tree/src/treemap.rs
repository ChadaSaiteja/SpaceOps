//! Squarified Treemap Layout Engine (ADR-010).
//!
//! Implements the Bruls-Huizing-van Wijk squarified treemap algorithm to partition
//! 2D screen space proportional to disk usage, minimizing aspect ratio distortion.

use crate::tree::{NodeKind, StorageTree};
use common::NodeId;

/// High-level file categorization based on extension (PRD §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum FileCategory {
    Video = 0,
    Audio = 1,
    Image = 2,
    Document = 3,
    Archive = 4,
    Executable = 5,
    Code = 6,
    System = 7,
    Other = 8,
}

impl FileCategory {
    pub fn from_i32(val: i32) -> Self {
        match val {
            0 => FileCategory::Video,
            1 => FileCategory::Audio,
            2 => FileCategory::Image,
            3 => FileCategory::Document,
            4 => FileCategory::Archive,
            5 => FileCategory::Executable,
            6 => FileCategory::Code,
            7 => FileCategory::System,
            _ => FileCategory::Other,
        }
    }
}

/// Classifies a file extension into a `FileCategory`.
pub fn classify_extension(ext: Option<&str>) -> FileCategory {
    match ext {
        Some(e) => {
            // Lowercase into a small stack buffer: this runs once per node for search and
            // layout, so a heap allocation here dominated both. No recognised extension
            // is longer than 7 bytes, so anything longer is `Other`.
            let bytes = e.as_bytes();
            let mut buf = [0u8; 8];
            if bytes.len() > buf.len() {
                return FileCategory::Other;
            }
            for (dst, src) in buf.iter_mut().zip(bytes) {
                *dst = src.to_ascii_lowercase();
            }
            let lower = match std::str::from_utf8(&buf[..bytes.len()]) {
                Ok(s) => s,
                Err(_) => return FileCategory::Other,
            };
            match lower {
                // Video
                "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg" | "mpeg"
                | "3gp" | "m2ts" | "vob" => FileCategory::Video,

                // Audio
                "mp3" | "wav" | "flac" | "aac" | "ogg" | "wma" | "m4a" | "aiff" | "mid"
                | "midi" | "opus" => FileCategory::Audio,

                // Images
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tiff"
                | "tif" | "heic" | "raw" | "psd" | "ai" => FileCategory::Image,

                // Documents
                "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf"
                | "csv" | "md" | "odt" | "ods" | "odp" | "epub" => FileCategory::Document,

                // Archives
                "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "cab" | "dmg"
                | "tgz" | "zst" => FileCategory::Archive,

                // Executables & Binaries / Installers
                "exe" | "msi" | "dll" | "sys" | "com" | "bat" | "cmd" | "ps1" | "vbs" | "scr"
                | "drv" | "ocx" => FileCategory::Executable,

                // Code & Developer Data
                "rs" | "cs" | "js" | "ts" | "jsx" | "tsx" | "py" | "c" | "cpp" | "h" | "hpp"
                | "java" | "go" | "html" | "css" | "json" | "xml" | "yaml" | "yml" | "sql"
                | "sh" | "toml" | "lock" | "props" | "targets" | "xaml" => FileCategory::Code,

                // System & Metadata
                "log" | "dat" | "ini" | "cfg" | "tmp" | "bak" | "dmp" | "evtx" | "reg" => {
                    FileCategory::System
                }

                _ => FileCategory::Other,
            }
        }
        None => FileCategory::Other,
    }
}

/// A single computed rectangle in the treemap layout.
#[derive(Debug, Clone, PartialEq)]
pub struct TreemapRect {
    pub node_id: NodeId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: u32,
    pub kind: NodeKind,
    pub category: FileCategory,
}

#[derive(Clone, Copy, Debug)]
struct RectBounds {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl RectBounds {
    fn shorter_side(&self) -> f32 {
        self.w.min(self.h)
    }

    fn area(&self) -> f32 {
        (self.w * self.h).max(0.0)
    }
}

/// Item to be placed by the squarify algorithm.
#[derive(Clone, Copy, Debug)]
struct LayoutItem {
    node_id: NodeId,
    kind: NodeKind,
    category: FileCategory,
    size: u64,
    normalized_area: f32,
}

/// Computes the complete hierarchical squarified treemap layout starting from `root_id`.
pub fn compute_treemap_layout(
    tree: &StorageTree,
    root_id: NodeId,
    width: f32,
    height: f32,
    max_depth: u32,
    min_size_px: f32,
) -> Vec<TreemapRect> {
    if width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }

    let root_node = match tree.node(root_id) {
        Some(n) => n,
        None => return Vec::new(),
    };

    let mut out_rects = Vec::with_capacity(512);

    // If root has no children or is a file, emit root itself
    let children_ids = match tree.children(root_id) {
        Some(c) if !c.is_empty() => c,
        _ => {
            let cat = classify_extension(root_node.extension);
            out_rects.push(TreemapRect {
                node_id: root_id,
                x: 0.0,
                y: 0.0,
                width,
                height,
                depth: 0,
                kind: root_node.kind,
                category: cat,
            });
            return out_rects;
        }
    };

    let bounds = RectBounds {
        x: 0.0,
        y: 0.0,
        w: width,
        h: height,
    };

    layout_children(
        tree,
        children_ids,
        bounds,
        1, // children are at depth 1 relative to root
        max_depth,
        min_size_px,
        &mut out_rects,
    );

    out_rects
}

fn layout_children(
    tree: &StorageTree,
    child_ids: &[NodeId],
    bounds: RectBounds,
    current_depth: u32,
    max_depth: u32,
    min_size_px: f32,
    out: &mut Vec<TreemapRect>,
) {
    if bounds.w < min_size_px || bounds.h < min_size_px || child_ids.is_empty() {
        return;
    }

    // Collect child metadata
    let mut items = Vec::with_capacity(child_ids.len());
    let mut total_size = 0u64;

    for &id in child_ids {
        if let Some(node) = tree.node(id) {
            // Directory categories are resolved lazily in `layout_row`, only for tiles that
            // survive culling, so the largest-child walk never runs for invisible items.
            let cat = if node.kind == NodeKind::Directory {
                FileCategory::Other
            } else {
                classify_extension(node.extension)
            };
            // Ensure even 0-byte files have at least 1 unit of weight so they don't vanish if lonely
            let effective_size = node.size.max(1);
            total_size += effective_size;
            items.push(LayoutItem {
                node_id: id,
                kind: node.kind,
                category: cat,
                size: effective_size,
                normalized_area: 0.0,
            });
        }
    }

    if items.is_empty() || total_size == 0 {
        return;
    }

    // Sort descending by size (they are already sorted in tree, but ensure consistency)
    items.sort_by_key(|a| std::cmp::Reverse(a.size));

    // Normalize areas to fit the total bounding area
    let total_area = bounds.area();
    for item in &mut items {
        item.normalized_area = (item.size as f32 / total_size as f32) * total_area;
    }

    // Run the squarify tiling algorithm
    squarify(
        tree,
        &items,
        bounds,
        current_depth,
        max_depth,
        min_size_px,
        out,
    );
}

/// Maximum number of largest-child hops `dominant_category` follows before giving up.
const MAX_DOMINANT_HOPS: usize = 32;

/// Cheap approximation of a directory's dominant content category: follows the largest
/// child (children are pre-sorted by size descending) until it reaches a file, and returns
/// that file's category. Empty directories, reparse points, inaccessible nodes and chains
/// longer than `MAX_DOMINANT_HOPS` yield `Other`. O(depth), never walks a whole subtree.
fn dominant_category(tree: &StorageTree, dir_id: NodeId) -> FileCategory {
    let mut current = dir_id;
    for _ in 0..=MAX_DOMINANT_HOPS {
        let node = match tree.node(current) {
            Some(n) => n,
            None => return FileCategory::Other,
        };
        match node.kind {
            NodeKind::File => return classify_extension(node.extension),
            NodeKind::Directory => match tree.children(current).and_then(|c| c.first()) {
                Some(&largest) => current = largest,
                None => return FileCategory::Other,
            },
            _ => return FileCategory::Other,
        }
    }
    FileCategory::Other
}

/// Squarifies a list of items into `bounds`.
fn squarify(
    tree: &StorageTree,
    items: &[LayoutItem],
    mut bounds: RectBounds,
    current_depth: u32,
    max_depth: u32,
    min_size_px: f32,
    out: &mut Vec<TreemapRect>,
) {
    if items.is_empty() || bounds.w < min_size_px || bounds.h < min_size_px {
        return;
    }

    let mut start_idx = 0;

    while start_idx < items.len() {
        let shorter = bounds.shorter_side();
        if shorter <= 0.001 {
            break;
        }

        // Items are sorted by area descending, so the worst aspect ratio of a row is
        // always attained at its largest (first) or smallest counted (last) item; track
        // those two instead of rescanning the whole row for every candidate item.
        let mut row_len = 1;
        let mut row_sum = items[start_idx].normalized_area;
        let max_area = items[start_idx].normalized_area;
        let mut min_area = max_area;
        let mut current_worst = worst_aspect_from_extremes(max_area, min_area, row_sum, shorter);

        while start_idx + row_len < items.len() {
            let next_item = &items[start_idx + row_len];
            let next_sum = row_sum + next_item.normalized_area;
            let next_min = if next_item.normalized_area > MIN_COUNTED_AREA {
                next_item.normalized_area
            } else {
                min_area
            };
            let next_worst = worst_aspect_from_extremes(max_area, next_min, next_sum, shorter);

            // If aspect ratio improves or stays the same, add to row
            if next_worst <= current_worst {
                row_sum = next_sum;
                min_area = next_min;
                current_worst = next_worst;
                row_len += 1;
            } else {
                break;
            }
        }

        // Layout this finalized row along the shorter edge
        let row_items = &items[start_idx..start_idx + row_len];
        layout_row(
            tree,
            row_items,
            row_sum,
            &mut bounds,
            current_depth,
            max_depth,
            min_size_px,
            out,
        );

        start_idx += row_len;
    }
}

/// Height reserved at the top of a directory tile for its label when it has children laid out.
const HEADER_HEIGHT: f32 = 18.0;
/// Minimum directory tile size that gets a label header; smaller tiles keep the 2px padding.
const HEADER_MIN_WIDTH: f32 = 60.0;
const HEADER_MIN_HEIGHT: f32 = 48.0;

/// Items with an area at or below this are ignored by the aspect-ratio computation.
const MIN_COUNTED_AREA: f32 = 0.0001;

/// Worst aspect ratio of a row given only its largest area `max_area` and smallest
/// counted area `min_area` (the row is sorted descending). The aspect of an item is
/// `max(l^2 a / s^2, s^2 / (l^2 a))`; the first term is monotone increasing and the
/// second monotone decreasing in `a`, so the row maximum is
/// `max(first_term(max_area), second_term(min_area))` — bit-identical to scanning the row.
fn worst_aspect_from_extremes(max_area: f32, min_area: f32, row_sum: f32, length: f32) -> f32 {
    if row_sum <= 0.0 || length <= 0.0 || max_area <= MIN_COUNTED_AREA {
        return f32::MAX;
    }
    let length_sq = length * length;
    let sum_sq = row_sum * row_sum;
    let r1 = (length_sq * max_area) / sum_sq;
    let r2 = sum_sq / (length_sq * min_area);
    let worst = r1.max(r2);
    if worst == 0.0 {
        f32::MAX
    } else {
        worst
    }
}

/// Reference implementation scanning the whole row; kept to verify
/// `worst_aspect_from_extremes` in tests.
#[cfg(test)]
fn worst_aspect_ratio(row: &[LayoutItem], row_sum: f32, length: f32) -> f32 {
    if row.is_empty() || row_sum <= 0.0 || length <= 0.0 {
        return f32::MAX;
    }

    let length_sq = length * length;
    let sum_sq = row_sum * row_sum;

    let mut max_aspect = 0.0f32;
    for item in row {
        let a = item.normalized_area;
        if a <= 0.0001 {
            continue;
        }
        // aspect = max( (length^2 * a) / sum^2, sum^2 / (length^2 * a) )
        let r1 = (length_sq * a) / sum_sq;
        let r2 = sum_sq / (length_sq * a);
        let aspect = r1.max(r2);
        if aspect > max_aspect {
            max_aspect = aspect;
        }
    }

    if max_aspect == 0.0 {
        f32::MAX
    } else {
        max_aspect
    }
}

/// Lays out a finalized row into `bounds`, then shrinks `bounds` by the row's thickness.
#[allow(clippy::too_many_arguments)]
fn layout_row(
    tree: &StorageTree,
    row: &[LayoutItem],
    row_sum: f32,
    bounds: &mut RectBounds,
    current_depth: u32,
    max_depth: u32,
    min_size_px: f32,
    out: &mut Vec<TreemapRect>,
) {
    if row.is_empty() || row_sum <= 0.0 {
        return;
    }

    let is_horizontal = bounds.w <= bounds.h;
    let row_thickness =
        (row_sum / bounds.shorter_side()).min(if is_horizontal { bounds.h } else { bounds.w });

    if row_thickness <= 0.001 {
        return;
    }

    let mut offset = 0.0f32;

    for item in row {
        let item_len = if row_sum > 0.0 {
            (item.normalized_area / row_sum) * bounds.shorter_side()
        } else {
            0.0
        };

        let (rx, ry, rw, rh) = if is_horizontal {
            // Horizontal row spanning along x, stacked along y
            (bounds.x + offset, bounds.y, item_len, row_thickness)
        } else {
            // Vertical column spanning along y, stacked along x
            (bounds.x, bounds.y + offset, row_thickness, item_len)
        };

        offset += item_len;

        // Skip items below pixel culling threshold
        if rw < min_size_px || rh < min_size_px {
            continue;
        }

        // Add this item rectangle
        let category = if item.kind == NodeKind::Directory {
            dominant_category(tree, item.node_id)
        } else {
            item.category
        };
        out.push(TreemapRect {
            node_id: item.node_id,
            x: rx,
            y: ry,
            width: rw,
            height: rh,
            depth: current_depth,
            kind: item.kind,
            category,
        });

        // Recursively subdivide directory children if within max_depth and large enough
        if item.kind == NodeKind::Directory
            && current_depth < max_depth
            && rw >= min_size_px * 3.0
            && rh >= min_size_px * 3.0
        {
            if let Some(sub_children) = tree.children(item.node_id) {
                if !sub_children.is_empty() {
                    // Small padding inside directory tile, plus a header strip on tiles big
                    // enough to label so the folder name stays visible above its children
                    let pad = 2.0f32;
                    let header = if rw >= HEADER_MIN_WIDTH && rh >= HEADER_MIN_HEIGHT {
                        HEADER_HEIGHT
                    } else {
                        pad
                    };
                    let inner_bounds = RectBounds {
                        x: rx + pad,
                        y: ry + header,
                        w: (rw - pad * 2.0).max(0.0),
                        h: (rh - header - pad).max(0.0),
                    };

                    layout_children(
                        tree,
                        sub_children,
                        inner_bounds,
                        current_depth + 1,
                        max_depth,
                        min_size_px,
                        out,
                    );
                }
            }
        }
    }

    // Shrink remaining bounds
    if is_horizontal {
        bounds.y += row_thickness;
        bounds.h = (bounds.h - row_thickness).max(0.0);
    } else {
        bounds.x += row_thickness;
        bounds.w = (bounds.w - row_thickness).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{DirMetadata, FileAttributeFlags, FileMetadata, NodeId, ScanEvent};
    use std::ffi::OsString;

    #[test]
    fn worst_aspect_from_extremes_matches_full_row_scan() {
        let mut seed = 99u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 40) as f32 / 1000.0
        };
        for _ in 0..200 {
            let len = 1 + (next() as usize % 40);
            let mut areas: Vec<f32> = (0..len)
                .map(|_| {
                    if (next() as u32 % 7) < 1 {
                        0.00005
                    } else {
                        next()
                    }
                })
                .collect();
            areas.sort_by(|a, b| b.partial_cmp(a).unwrap());
            let items: Vec<LayoutItem> = areas
                .iter()
                .map(|&a| LayoutItem {
                    node_id: NodeId(0),
                    kind: NodeKind::File,
                    category: FileCategory::Other,
                    size: 1,
                    normalized_area: a,
                })
                .collect();
            let length = 1.0 + next();
            for end in 1..=items.len() {
                let row = &items[..end];
                let sum: f32 = row.iter().map(|i| i.normalized_area).sum();
                let min_counted = row
                    .iter()
                    .map(|i| i.normalized_area)
                    .rfind(|a| *a > MIN_COUNTED_AREA)
                    .unwrap_or(row[0].normalized_area);
                assert_eq!(
                    worst_aspect_ratio(row, sum, length),
                    worst_aspect_from_extremes(row[0].normalized_area, min_counted, sum, length)
                );
            }
        }
    }

    #[test]
    fn classify_extension_is_case_insensitive_and_bounded() {
        assert_eq!(classify_extension(Some("MP4")), FileCategory::Video);
        assert_eq!(classify_extension(Some("TaRgEtS")), FileCategory::Code);
        assert_eq!(classify_extension(Some("")), FileCategory::Other);
        assert_eq!(
            classify_extension(Some("averyverylongext")),
            FileCategory::Other
        );
        assert_eq!(classify_extension(Some("m\u{4f60}p")), FileCategory::Other);
        assert_eq!(classify_extension(None), FileCategory::Other);
    }

    fn build_test_tree() -> StorageTree {
        let root_id = NodeId(0);
        let sub_id = NodeId(1);

        let events = vec![
            ScanEvent::EnteredDirectory {
                id: root_id,
                parent_id: None,
                meta: DirMetadata {
                    name: OsString::from("root"),
                    is_reparse_point: false,
                },
            },
            ScanEvent::FileFound {
                parent_id: root_id,
                meta: FileMetadata {
                    name: OsString::from("video.mp4"),
                    size: 6000,
                    extension: Some("mp4".to_string()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::FileFound {
                parent_id: root_id,
                meta: FileMetadata {
                    name: OsString::from("music.mp3"),
                    size: 2000,
                    extension: Some("mp3".to_string()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::EnteredDirectory {
                id: sub_id,
                parent_id: Some(root_id),
                meta: DirMetadata {
                    name: OsString::from("docs"),
                    is_reparse_point: false,
                },
            },
            ScanEvent::FileFound {
                parent_id: sub_id,
                meta: FileMetadata {
                    name: OsString::from("report.pdf"),
                    size: 2000,
                    extension: Some("pdf".to_string()),
                    modified: None,
                    created: None,
                    attributes: FileAttributeFlags::default(),
                },
            },
            ScanEvent::DirectoryComplete {
                id: sub_id,
                total_size: 2000,
                file_count: 1,
                dir_count: 0,
            },
            ScanEvent::DirectoryComplete {
                id: root_id,
                total_size: 10000,
                file_count: 3,
                dir_count: 1,
            },
        ];

        StorageTree::build(events).unwrap()
    }

    #[test]
    fn classify_extensions_correctly() {
        assert_eq!(classify_extension(Some("mp4")), FileCategory::Video);
        assert_eq!(classify_extension(Some("MP3")), FileCategory::Audio);
        assert_eq!(classify_extension(Some("png")), FileCategory::Image);
        assert_eq!(classify_extension(Some("pdf")), FileCategory::Document);
        assert_eq!(classify_extension(Some("zip")), FileCategory::Archive);
        assert_eq!(classify_extension(Some("exe")), FileCategory::Executable);
        assert_eq!(classify_extension(Some("rs")), FileCategory::Code);
        assert_eq!(classify_extension(Some("log")), FileCategory::System);
        assert_eq!(classify_extension(Some("unknown")), FileCategory::Other);
        assert_eq!(classify_extension(None), FileCategory::Other);
    }

    #[test]
    fn compute_layout_produces_valid_rectangles() {
        let tree = build_test_tree();
        let rects = compute_treemap_layout(&tree, tree.root(), 800.0, 600.0, 2, 2.0);

        assert!(!rects.is_empty());
        for r in &rects {
            assert!(r.x >= 0.0);
            assert!(r.y >= 0.0);
            assert!(
                r.x + r.width <= 800.5,
                "x+w exceeds viewport: {}",
                r.x + r.width
            );
            assert!(
                r.y + r.height <= 600.5,
                "y+h exceeds viewport: {}",
                r.y + r.height
            );
            assert!(r.width >= 2.0);
            assert!(r.height >= 2.0);
        }

        // Verify that video.mp4 (size 6000 out of 10000) has largest area
        let video_rect = rects
            .iter()
            .find(|r| r.category == FileCategory::Video)
            .expect("video rect missing");
        let video_area = video_rect.width * video_rect.height;
        let total_viewport_area = 800.0 * 600.0;
        // video should take approximately 50-60% of total area
        assert!(
            video_area > total_viewport_area * 0.45,
            "Video area {} should be around 60% of viewport {}",
            video_area,
            total_viewport_area
        );
    }

    #[test]
    fn depth_limit_is_respected() {
        let tree = build_test_tree();
        // With max_depth = 1, sub's children should not be laid out
        let rects_d1 = compute_treemap_layout(&tree, tree.root(), 800.0, 600.0, 1, 2.0);
        assert!(rects_d1.iter().all(|r| r.depth <= 1));

        // With max_depth = 2, sub's child (report.pdf at depth 2) should be included
        let rects_d2 = compute_treemap_layout(&tree, tree.root(), 800.0, 600.0, 2, 2.0);
        let has_depth_2 = rects_d2.iter().any(|r| r.depth == 2);
        assert!(has_depth_2);
    }

    #[test]
    fn culling_threshold_filters_tiny_tiles() {
        let tree = build_test_tree();
        // Very high culling threshold of 350px
        let rects = compute_treemap_layout(&tree, tree.root(), 800.0, 600.0, 2, 350.0);
        for r in &rects {
            assert!(r.width >= 350.0 || r.height >= 350.0);
        }
    }

    fn dir_event(id: u64, parent: Option<u64>, name: &str) -> ScanEvent {
        ScanEvent::EnteredDirectory {
            id: NodeId(id),
            parent_id: parent.map(NodeId),
            meta: DirMetadata {
                name: OsString::from(name),
                is_reparse_point: false,
            },
        }
    }

    fn file_event(parent: u64, name: &str, ext: &str, size: u64) -> ScanEvent {
        ScanEvent::FileFound {
            parent_id: NodeId(parent),
            meta: FileMetadata {
                name: OsString::from(name),
                size,
                extension: Some(ext.to_string()),
                modified: None,
                created: None,
                attributes: FileAttributeFlags::default(),
            },
        }
    }

    fn done_event(id: u64, total_size: u64, file_count: u64, dir_count: u64) -> ScanEvent {
        ScanEvent::DirectoryComplete {
            id: NodeId(id),
            total_size,
            file_count,
            dir_count,
        }
    }

    /// root/
    ///   media/            (mostly video, nested one level)
    ///     season1/ ep1.mkv 7000, ep2.mkv 1000
    ///     notes.txt 100
    ///   empty/
    ///   misc/ blob.unknownext 900
    fn build_category_tree() -> StorageTree {
        let events = vec![
            dir_event(0, None, "root"),
            dir_event(1, Some(0), "media"),
            dir_event(2, Some(1), "season1"),
            file_event(2, "ep1.mkv", "mkv", 7000),
            file_event(2, "ep2.mkv", "mkv", 1000),
            done_event(2, 8000, 2, 0),
            file_event(1, "notes.txt", "txt", 100),
            done_event(1, 8100, 3, 1),
            dir_event(3, Some(0), "empty"),
            done_event(3, 0, 0, 0),
            dir_event(4, Some(0), "misc"),
            file_event(4, "blob.unknownext", "unknownext", 900),
            done_event(4, 900, 1, 0),
            done_event(0, 9000, 4, 4),
        ];
        StorageTree::build(events).unwrap()
    }

    /// Tree ids are reassigned during build, so tests look directories up by name.
    fn child_named(tree: &StorageTree, parent: NodeId, name: &str) -> NodeId {
        *tree
            .children(parent)
            .unwrap()
            .iter()
            .find(|&&c| tree.node(c).unwrap().name == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    }

    #[test]
    fn directory_with_mostly_video_is_categorized_video() {
        let tree = build_category_tree();
        let media_id = child_named(&tree, tree.root(), "media");
        assert_eq!(dominant_category(&tree, media_id), FileCategory::Video);

        let rects = compute_treemap_layout(&tree, tree.root(), 800.0, 600.0, 1, 2.0);
        let media = rects
            .iter()
            .find(|r| r.node_id == media_id)
            .expect("media rect missing");
        assert_eq!(media.kind, NodeKind::Directory);
        assert_eq!(media.category, FileCategory::Video);
    }

    #[test]
    fn empty_and_unknown_directories_stay_other() {
        let tree = build_category_tree();
        let empty = child_named(&tree, tree.root(), "empty");
        let misc = child_named(&tree, tree.root(), "misc");
        assert_eq!(dominant_category(&tree, empty), FileCategory::Other);
        assert_eq!(dominant_category(&tree, misc), FileCategory::Other);
    }

    #[test]
    fn dominant_category_stops_after_hop_limit() {
        // A chain of directories deeper than the hop limit ending in a video file.
        let depth = MAX_DOMINANT_HOPS as u64 + 5;
        let mut events = Vec::new();
        events.push(dir_event(0, None, "root"));
        for i in 1..=depth {
            events.push(dir_event(i, Some(i - 1), &format!("d{i}")));
        }
        events.push(file_event(depth, "deep.mp4", "mp4", 500));
        for i in (0..=depth).rev() {
            events.push(done_event(i, 500, 1, u64::from(i < depth)));
        }
        let tree = StorageTree::build(events).unwrap();
        assert_eq!(dominant_category(&tree, tree.root()), FileCategory::Other);
        // Close enough to the file, the chain resolves.
        let mut near = tree.root();
        for i in 1..=depth - 3 {
            near = child_named(&tree, near, &format!("d{i}"));
        }
        assert_eq!(dominant_category(&tree, near), FileCategory::Video);
    }

    #[test]
    fn zero_viewport_returns_empty() {
        let tree = build_test_tree();
        assert!(compute_treemap_layout(&tree, tree.root(), 0.0, 600.0, 2, 2.0).is_empty());
        assert!(compute_treemap_layout(&tree, tree.root(), 800.0, 0.0, 2, 2.0).is_empty());
        assert!(compute_treemap_layout(&tree, tree.root(), -10.0, 600.0, 2, 2.0).is_empty());
    }
}
