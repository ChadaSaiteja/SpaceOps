//! Squarified Treemap Layout Engine (ADR-010).
//!
//! Implements the Bruls-Huizing-van Wijk squarified treemap algorithm to partition
//! 2D screen space proportional to disk usage, minimizing aspect ratio distortion.

use common::NodeId;
use crate::tree::{NodeKind, StorageTree};

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
            let lower = e.to_ascii_lowercase();
            match lower.as_str() {
                // Video
                "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "mpg"
                | "mpeg" | "3gp" | "m2ts" | "vob" => FileCategory::Video,

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
                "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "cab"
                | "dmg" | "tgz" | "zst" => FileCategory::Archive,

                // Executables & Binaries / Installers
                "exe" | "msi" | "dll" | "sys" | "com" | "bat" | "cmd" | "ps1" | "vbs"
                | "scr" | "drv" | "ocx" => FileCategory::Executable,

                // Code & Developer Data
                "rs" | "cs" | "js" | "ts" | "jsx" | "tsx" | "py" | "c" | "cpp" | "h"
                | "hpp" | "java" | "go" | "html" | "css" | "json" | "xml" | "yaml"
                | "yml" | "sql" | "sh" | "toml" | "lock" | "props" | "targets"
                | "xaml" => FileCategory::Code,

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
    items.sort_by(|a, b| b.size.cmp(&a.size));

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

        let mut row_len = 1;
        let mut row_sum = items[start_idx].normalized_area;
        let mut current_worst = worst_aspect_ratio(&items[start_idx..start_idx + 1], row_sum, shorter);

        while start_idx + row_len < items.len() {
            let next_item = &items[start_idx + row_len];
            let next_sum = row_sum + next_item.normalized_area;
            let next_worst = worst_aspect_ratio(
                &items[start_idx..start_idx + row_len + 1],
                next_sum,
                shorter,
            );

            // If aspect ratio improves or stays the same, add to row
            if next_worst <= current_worst {
                row_sum = next_sum;
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

/// Computes the worst aspect ratio among items in a row along edge length `length`.
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
    let row_thickness = (row_sum / bounds.shorter_side()).min(if is_horizontal {
        bounds.h
    } else {
        bounds.w
    });

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
        out.push(TreemapRect {
            node_id: item.node_id,
            x: rx,
            y: ry,
            width: rw,
            height: rh,
            depth: current_depth,
            kind: item.kind,
            category: item.category,
        });

        // Recursively subdivide directory children if within max_depth and large enough
        if item.kind == NodeKind::Directory
            && current_depth < max_depth
            && rw >= min_size_px * 3.0
            && rh >= min_size_px * 3.0
        {
            if let Some(sub_children) = tree.children(item.node_id) {
                if !sub_children.is_empty() {
                    // Small padding inside directory tile
                    let pad = 2.0f32;
                    let inner_bounds = RectBounds {
                        x: rx + pad,
                        y: ry + pad,
                        w: (rw - pad * 2.0).max(0.0),
                        h: (rh - pad * 2.0).max(0.0),
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
            assert!(r.x + r.width <= 800.5, "x+w exceeds viewport: {}", r.x + r.width);
            assert!(r.y + r.height <= 600.5, "y+h exceeds viewport: {}", r.y + r.height);
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

    #[test]
    fn zero_viewport_returns_empty() {
        let tree = build_test_tree();
        assert!(compute_treemap_layout(&tree, tree.root(), 0.0, 600.0, 2, 2.0).is_empty());
        assert!(compute_treemap_layout(&tree, tree.root(), 800.0, 0.0, 2, 2.0).is_empty());
        assert!(compute_treemap_layout(&tree, tree.root(), -10.0, 600.0, 2, 2.0).is_empty());
    }
}
