using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;

namespace StorageIntelligence.Services;

/// <summary>Share of the scanned tree attributed to one file category (or the smaller-files remainder).</summary>
public sealed record CategorySlice(FileCategory? Category, string Name, ulong Bytes, double Fraction);

/// <summary>One of the biggest direct children of the scan root.</summary>
public sealed record TopConsumer(ulong NodeId, string Name, NodeKind Kind, ulong Bytes, double Fraction);

public sealed record OverviewSnapshot(
    string RootName,
    ulong TotalBytes,
    ulong FileCount,
    ulong DirCount,
    ulong NodeCount,
    IReadOnlyList<CategorySlice> Slices,
    ulong SlicedFileCount,
    IReadOnlyList<TopConsumer> TopConsumers);

public sealed record ReclaimableSummary(ulong SystemJunkBytes, int SystemJunkCategories, ulong DevBytes, int DevArtifacts)
{
    public ulong TotalBytes => SystemJunkBytes + DevBytes;
}

/// <summary>
/// Computes the Overview page numbers from the in-memory storage tree (no extra disk access).
/// Category shares come from the largest files in the tree (exact sizes), with every remaining
/// byte reported as "Smaller files", so the slices always add up to the scanned total.
/// </summary>
public static class OverviewService
{
    /// <summary>How many of the largest files are classified by extension for the category donut.</summary>
    public const uint TopFilesSampled = 5000;

    public const int TopConsumerCount = 5;

    public static Task<OverviewSnapshot> ComputeAsync(StorageTreeService tree) =>
        Task.Run(() => Compute(tree));

    public static OverviewSnapshot Compute(StorageTreeService tree)
    {
        ulong rootId = tree.GetRootId();
        var root = tree.GetNodeInfo(rootId, includeFullPath: true);
        ulong total = root.Size;

        // Category donut: classify the largest files exactly.
        var bytesByCategory = new Dictionary<FileCategory, ulong>();
        ulong sampledBytes = 0;
        var topFileIds = tree.GetTopFilesBySize(rootId, TopFilesSampled);
        foreach (ulong id in topFileIds)
        {
            var info = tree.GetNodeInfo(id);
            if (info.Kind != NodeKind.File) continue;

            string ext = System.IO.Path.GetExtension(info.Name);
            var cat = TreemapService.ClassifyExtension(ext);
            bytesByCategory[cat] = bytesByCategory.GetValueOrDefault(cat) + info.Size;
            sampledBytes += info.Size;
        }

        var slices = new List<CategorySlice>();
        foreach (var (cat, bytes) in bytesByCategory.OrderByDescending(kv => kv.Value))
        {
            slices.Add(new CategorySlice(cat, Styles.Palette.CategoryName(cat), bytes, Fraction(bytes, total)));
        }

        ulong remainder = total > sampledBytes ? total - sampledBytes : 0;
        if (remainder > 0)
        {
            slices.Add(new CategorySlice(null, "Smaller files", remainder, Fraction(remainder, total)));
        }

        // Top consumers: children are pre-sorted by size descending by the core.
        var consumers = new List<TopConsumer>();
        if (root.Kind == NodeKind.Directory)
        {
            var children = tree.GetChildren(rootId, 0, TopConsumerCount);
            foreach (ulong childId in children.NodeIds)
            {
                var child = tree.GetNodeInfo(childId);
                consumers.Add(new TopConsumer(child.Id, child.Name, child.Kind, child.Size, Fraction(child.Size, total)));
            }
        }

        string rootName = string.IsNullOrEmpty(root.FullPath) ? root.Name : root.FullPath;
        return new OverviewSnapshot(
            rootName,
            total,
            root.FileCount,
            root.DirCount,
            tree.GetNodeCount(),
            slices,
            (ulong)topFileIds.Length,
            consumers);
    }

    /// <summary>Reclaimable-now: unprotected cleanup candidates plus dormant, directly cleanable developer artifacts.</summary>
    public static async Task<ReclaimableSummary> ComputeReclaimableAsync(StorageTreeService tree)
    {
        var candidates = await tree.DetectCleanupCandidatesAsync();
        ulong junk = 0;
        int junkCategories = 0;
        foreach (var c in candidates)
        {
            if (c.IsProtected || c.TotalBytes == 0) continue;
            junk += c.TotalBytes;
            junkCategories++;
        }

        var artifacts = await DevStorageService.DiscoverArtifactsAsync(tree);
        ulong dev = 0;
        int devCount = 0;
        foreach (var a in artifacts)
        {
            if (!a.IsDormant || a.RiskLevel == RiskLevel.High) continue;
            dev += a.SizeBytes;
            devCount++;
        }

        return new ReclaimableSummary(junk, junkCategories, dev, devCount);
    }

    private static double Fraction(ulong part, ulong total) =>
        total == 0 ? 0 : Math.Min(1.0, (double)part / total);
}
