using System;

namespace StorageIntelligence.Native;

/// <summary>
/// High-level file categorization based on extension (PRD §8, ADR-010).
/// </summary>
public enum FileCategory
{
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

/// <summary>
/// A laid-out treemap rectangle in viewport coordinates (ADR-010).
/// </summary>
public readonly record struct TreemapRect(
    ulong NodeId,
    float X,
    float Y,
    float Width,
    float Height,
    uint Depth,
    NodeKind Kind,
    FileCategory Category
)
{
    /// <summary>
    /// Checks whether point (px, py) lies inside this rectangle.
    /// </summary>
    public bool Contains(float px, float py) =>
        px >= X && px <= (X + Width) && py >= Y && py <= (Y + Height);
}

/// <summary>
/// Service for computing and querying treemap layouts (ADR-010).
/// </summary>
public static unsafe class TreemapService
{
    /// <summary>
    /// Computes squarified treemap layout starting from <paramref name="rootId"/> for the given viewport.
    /// </summary>
    public static TreemapRect[] ComputeLayout(
        StorageTreeService tree,
        ulong rootId,
        float width,
        float height,
        uint maxDepth = 2,
        float minSizePx = 3.0f)
    {
        if (tree == null) throw new ArgumentNullException(nameof(tree));
        if (width <= 0 || height <= 0) return Array.Empty<TreemapRect>();

        uint initialCapacity = 2048;
        var buffer = new TreemapRectFfi[initialCapacity];
        uint count = 0;
        uint total = 0;

        fixed (TreemapRectFfi* ptr = buffer)
        {
            int code = NativeMethods.tree_compute_layout(
                tree.Handle.DangerousHandle,
                rootId,
                width,
                height,
                maxDepth,
                minSizePx,
                ptr,
                initialCapacity,
                &count,
                &total);

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to compute treemap layout for root {rootId}");
            }
        }

        // If buffer was too small to hold all generated rectangles, reallocate to exact total
        if (total > initialCapacity)
        {
            buffer = new TreemapRectFfi[total];
            fixed (TreemapRectFfi* ptr = buffer)
            {
                int code = NativeMethods.tree_compute_layout(
                    tree.Handle.DangerousHandle,
                    rootId,
                    width,
                    height,
                    maxDepth,
                    minSizePx,
                    ptr,
                    total,
                    &count,
                    &total);

                if (code != 0)
                {
                    throw new ScanException(code, $"Failed to recompute treemap layout for root {rootId}");
                }
            }
        }

        var result = new TreemapRect[count];
        for (int i = 0; i < count; i++)
        {
            ref readonly var raw = ref buffer[i];
            result[i] = new TreemapRect(
                raw.node_id,
                raw.x,
                raw.y,
                raw.width,
                raw.height,
                raw.depth,
                (NodeKind)raw.kind,
                (FileCategory)raw.category);
        }

        return result;
    }

    /// <summary>
    /// Performs point-in-rect hit testing. If nested rectangles overlap, returns the deepest
    /// (most specific leaf/child) node.
    /// </summary>
    public static TreemapRect? FindNodeAtPoint(float px, float py, ReadOnlySpan<TreemapRect> rects)
    {
        TreemapRect? bestMatch = null;
        uint maxDepth = 0;

        for (int i = 0; i < rects.Length; i++)
        {
            ref readonly var r = ref rects[i];
            if (r.Contains(px, py))
            {
                if (bestMatch == null || r.Depth >= maxDepth)
                {
                    bestMatch = r;
                    maxDepth = r.Depth;
                }
            }
        }

        return bestMatch;
    }

    /// <summary>
    /// Returns default hex color for category per ADR-010.
    /// </summary>
    public static string GetCategoryColorHex(FileCategory category) => category switch
    {
        FileCategory.Video => "#8A2BE2",       // Purple
        FileCategory.Audio => "#FF1493",       // Pink
        FileCategory.Image => "#FF8C00",       // Amber/Orange
        FileCategory.Document => "#1E90FF",    // Blue
        FileCategory.Archive => "#32CD32",     // Lime Green
        FileCategory.Executable => "#DC143C",  // Crimson
        FileCategory.Code => "#00CED1",        // Cyan
        FileCategory.System => "#708090",      // Slate Gray
        _ => "#696969",                        // Dim Gray / Other
    };

    /// <summary>
    /// Classifies an extension string into a FileCategory (PRD §8).
    /// </summary>
    public static FileCategory ClassifyExtension(string? ext)
    {
        if (string.IsNullOrEmpty(ext)) return FileCategory.Other;
        string lower = ext.TrimStart('.').ToLowerInvariant();

        return lower switch
        {
            "mp4" or "mkv" or "avi" or "mov" or "wmv" or "flv" or "webm" or "m4v" or "mpg" or "mpeg" or "3gp" or "m2ts" or "vob"
                => FileCategory.Video,

            "mp3" or "wav" or "flac" or "aac" or "ogg" or "wma" or "m4a" or "aiff" or "mid" or "midi" or "opus"
                => FileCategory.Audio,

            "png" or "jpg" or "jpeg" or "gif" or "bmp" or "webp" or "svg" or "ico" or "tiff" or "tif" or "heic" or "raw" or "psd" or "ai"
                => FileCategory.Image,

            "pdf" or "doc" or "docx" or "xls" or "xlsx" or "ppt" or "pptx" or "txt" or "rtf" or "csv" or "md" or "odt" or "ods" or "odp" or "epub"
                => FileCategory.Document,

            "zip" or "rar" or "7z" or "tar" or "gz" or "bz2" or "xz" or "iso" or "cab" or "dmg" or "tgz" or "zst"
                => FileCategory.Archive,

            "exe" or "msi" or "dll" or "sys" or "com" or "bat" or "cmd" or "ps1" or "vbs" or "scr" or "drv" or "ocx"
                => FileCategory.Executable,

            "rs" or "cs" or "js" or "ts" or "jsx" or "tsx" or "py" or "c" or "cpp" or "h" or "hpp" or "java" or "go" or "html" or "css" or "json" or "xml" or "yaml" or "yml" or "sql" or "sh" or "toml" or "lock" or "props" or "targets" or "xaml"
                => FileCategory.Code,

            "log" or "dat" or "ini" or "cfg" or "tmp" or "bak" or "dmp" or "evtx" or "reg"
                => FileCategory.System,

            _ => FileCategory.Other,
        };
    }
}
