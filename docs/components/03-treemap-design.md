# Treemap Visualization — Component Design

Status: **Approved. Implementation phase plan follows below.**

Governed by: ADR-001 (FFI), ADR-003 (module boundaries), ADR-004 (errors), ADR-007 (threading), ADR-009 (StorageTree arena), ADR-010 (treemap decisions).

---

## 1. Problem

Given a hierarchical `StorageTree` (Phase 3) that may contain up to 1,000,000+ files and directories, render an interactive, hardware-accelerated squarified treemap that visually partitions 2D screen space proportional to disk usage, providing instant sub-millisecond drill-down, breadcrumb navigation, selection, and hover inspections without UI stutters.

---

## 2. Requirements (PRD §7, §8, §21, §22)

- **Layout calculation:** Squarified tiling algorithm (Bruls-Huizing-van Wijk) minimizing aspect ratio distortion (ADR-010 #1, #3).
- **Interactive drill-down:** Double-clicking any directory node immediately sets it as the viewport root, recalculating the layout for its immediate subtree in $< 1\text{ ms}$ (ADR-010 #4, #6).
- **Breadcrumb navigation:** Clear breadcrumb trail above the treemap allowing 1-click navigation back to any ancestor up to the drive root.
- **Rendering performance:** 60/120 FPS hardware-accelerated rendering using Win2D (`Microsoft.Graphics.Win2D`) on a `CanvasControl` (ADR-010 #2).
- **Selection & hover:**
  - Hover: instant hit-testing displaying tooltip with name, size, percentage of parent, and path.
  - Selection: single-click highlights the node with a high-contrast border and informs the file/folder details panel.
- **Level of detail (LOD) & culling:** Culling threshold for small rectangles ($< 3\text{ px}$) and default visual depth limit of 2 levels below the active drill-down root.
- **Categorical color scheme:** Color coding based on file type/category (Videos, Audio, Documents, Images, Archives, Executables, Code, System, Other).

---

## 3. Architecture & Rust/UI Boundary

```
                     +---------------------------------------+
                     |         WinUI 3 UI Layer              |
                     |  TreemapView.xaml (Win2D CanvasControl)|
                     |  BreadcrumbBar, DetailsPane, Tooltip  |
                     +---------------------------------------+
                                        │
                         C# P/Invoke    │  Managed TreemapService
                                        ▼
                     +---------------------------------------+
                     |             ffi C-ABI                 |
                     |  tree_compute_layout()                |
                     |  tree_node_info(), tree_node_name()   |
                     +---------------------------------------+
                                        │
                                        ▼
                     +---------------------------------------+
                     |       storage-tree Crate (Core)       |
                     |  treemap.rs: Squarify Layout Engine   |
                     |  tree.rs: Arena-backed StorageTree    |
                     +---------------------------------------+
```

---

## 4. Detailed Component Models

### 4.1. Input Data Model
- Viewport dimensions: `width: f32`, `height: f32`.
- Active root: `root_id: NodeId` (defaults to tree root; updated during drill-down).
- Max depth: `max_depth: u32` (default: 2 levels).
- Culling threshold: `min_size_px: f32` (default: 3.0 px).

### 4.2. Layout Model (Squarified Tiling)
The squarified algorithm subdivides a target rectangle `(x, y, w, h)`:
1. Normalizes child node sizes relative to the total size of the current parent node.
2. Orders children descending by size.
3. Places children in rows along the shorter remaining side of the bounding box.
4. Adds items to the current row as long as the maximum aspect ratio of rectangles in the row improves (approaches 1.0).
5. Once adding an item worsens the aspect ratio, fixes the row and recursively squarifies the remaining bounding box with the rest of the children.
6. For directory children within `max_depth`, recursively subdivides their allotted rectangle for their own children.

Output per tile (`TreemapRectFfi`):
```rust
#[repr(C)]
pub struct TreemapRectFfi {
    pub node_id: u64,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub depth: u32,
    pub kind: i32,           // 0=Directory, 1=File, 2=ReparsePoint, 3=Inaccessible
    pub category: i32,       // 0=Video, 1=Audio, 2=Image, 3=Document, 4=Archive, 5=Executable, 6=Code, 7=System, 8=Other
}
```

### 4.3. Rendering Model (Win2D / CanvasControl)
- `CanvasControl_Draw` event renders directly to the Direct2D swapchain:
  - Fills each rectangle using category color brush (with slight luminance reduction for deeper nesting levels).
  - Draws inner borders separating sibling tiles (1px dark/neutral line).
  - Draws text label (filename or folder name) if rectangle width $> 40\text{ px}$ and height $> 18\text{ px}$, with clipping/ellipsis.
  - Draws selection outline (accent color, 2px stroke) around the selected node's rectangle.
  - Draws hover outline (white/accent highlight) around the hovered rectangle.

### 4.4. Interaction Model
- **PointerMoved:** Fast binary search / flat point-in-rect scan on the layout buffer ($< 50\text{ }\mu\text{s}$ for 1,000 rects). Updates hover state and triggers lightweight redraw (`Invalidate()`).
- **PointerPressed:** Selects target `node_id`. Dispatches `NodeSelected` event.
- **DoubleTapped:** If selected node is a directory, calls `DrillDown(node_id)`:
  - Updates active drill-down stack: `[Root, Subdir1, Subdir2]`.
  - Recalculates layout for new root.
  - Updates `BreadcrumbBar`.
- **Breadcrumb Click:** Navigates back up to the clicked ancestor node.

### 4.5. Categorical Color Mapping
| Category | File Extensions | Default Hex Color |
| :--- | :--- | :--- |
| **Video** | `.mp4`, `.mkv`, `.avi`, `.mov`, `.wmv` | `#8A2BE2` (Purple) |
| **Audio** | `.mp3`, `.wav`, `.flac`, `.aac`, `.m4a` | `#FF1493` (Pink) |
| **Images** | `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.svg` | `#FF8C00` (Amber) |
| **Documents** | `.pdf`, `.docx`, `.xlsx`, `.pptx`, `.txt`, `.csv` | `#1E90FF` (Blue) |
| **Archives** | `.zip`, `.rar`, `.7z`, `.tar`, `.gz`, `.iso` | `#32CD32` (Lime Green) |
| **Executables** | `.exe`, `.dll`, `.msi`, `.sys` | `#DC143C` (Crimson) |
| **Code** | `.rs`, `.cs`, `.js`, `.ts`, `.py`, `.cpp`, `.json` | `#00CED1` (Cyan) |
| **System** | System files / hidden / root system dirs | `#708090` (Slate Gray) |
| **Other** | All unclassified extensions | `#696969` (Dim Gray) |

---

## 5. Performance Strategy

1. **Sub-millisecond layout:** Rust computes layout in a single pass using pre-sorted children from Phase 3.
2. **Hardware acceleration:** Win2D renders via GPU drawing commands without COM object overhead per rectangle.
3. **Throttled resize:** Window resize updates layout with lightweight debouncing/requestAnimationFrame pattern.
4. **Memory:** The layout buffer for 1,000 tiles requires only $\approx 32\text{ KB}$ of memory.

---

## 6. Testing Strategy

1. **Rust unit tests (`storage-tree` crate):**
   - Squarified algorithm aspect ratio verification.
   - Depth limitation verification (stops at `max_depth`).
   - Culling verification (tiles smaller than threshold omitted).
   - Single file, flat directory, nested tree layout geometry correctness.
   - Point-in-rect hit testing logic.
2. **FFI integration tests (`ffi` crate):**
   - `tree_compute_layout` lifecycle and bounds checking.
   - Buffer overflow safety when limit is smaller than actual tiles.
3. **C# Integration & UI tests (`StorageIntelligence.IntegrationTests`):**
   - `TreemapService` layout roundtrip from real scan.
   - Hit-testing and drill-down state transitions.
