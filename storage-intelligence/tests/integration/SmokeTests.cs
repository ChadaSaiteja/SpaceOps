using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using StorageIntelligence.Services;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// End-to-end smoke check (roadmap v0.2, Task 0): the same pipeline the app runs after the user
/// presses Scan, driven headlessly against a temp folder this test creates:
/// scan -> tree -> treemap layout -> search -> developer storage -> unified cleanup analysis -> dry run.
/// Cleanup is only ever executed with dryRun: true, and only for candidates inside the temp folder.
/// </summary>
public class SmokeTests
{
    // Sizes are multiples of 4096 so logical size equals allocated size on 4 KiB-cluster NTFS.
    private static readonly (string RelativePath, int Size)[] Layout =
    {
        (@"docs\readme.txt", 4096),
        (@"docs\report.pdf", 16384),
        (@"media\clip.mp4", 65536),
        (@"media\photos\beach.jpg", 8192),
        (@"web\package.json", 4096),
        (@"web\node_modules\react\index.js", 32768),
        (@"web\node_modules\lodash\lodash.js", 12288),
        (@"engine\Cargo.toml", 4096),
        (@"engine\src\main.rs", 4096),
        (@"engine\target\release\engine.exe", 131072),
        (@"engine\target\release\deps\libengine.rlib", 49152),
    };

    // root, docs, media, photos, web, node_modules, react, lodash, engine, src, target, release, deps
    private const ulong ExpectedDirs = 13;

    private static DirectoryInfo CreateFixture()
    {
        var root = Directory.CreateTempSubdirectory("si_smoke_");
        foreach (var (rel, size) in Layout)
        {
            string full = Path.Combine(root.FullName, rel);
            Directory.CreateDirectory(Path.GetDirectoryName(full)!);
            File.WriteAllBytes(full, new byte[size]);
        }
        return root;
    }

    private static Dictionary<string, long> Snapshot(DirectoryInfo root) =>
        root.EnumerateFiles("*", SearchOption.AllDirectories)
            .ToDictionary(f => f.FullName, f => f.Length, StringComparer.OrdinalIgnoreCase);

    private static bool IsUnder(string path, string root)
    {
        string p = Path.GetFullPath(path).TrimEnd('\\') + "\\";
        string r = Path.GetFullPath(root).TrimEnd('\\') + "\\";
        return p.StartsWith(r, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task EndToEnd_ScanTreeTreemapSearch_OnTempFolder()
    {
        var root = CreateFixture();
        try
        {
            // 1. Scan + build tree (AppSession.ScanAsync path)
            var summary = await ScanService.ScanDriveAsync(root.FullName);
            using var scan = await ScanService.ScanDriveWithResultAsync(root.FullName);
            using var tree = await StorageTreeService.BuildAsync(scan);

            ulong expectedBytes = (ulong)Layout.Sum(l => (long)l.Size);
            Assert.Equal((ulong)Layout.Length, summary.TotalFiles);
            Assert.Equal(ExpectedDirs, summary.TotalDirs);
            Assert.Equal(0ul, summary.InaccessibleCount);
            Assert.True(summary.TotalSize >= expectedBytes, $"Scanned {summary.TotalSize} < expected {expectedBytes}");

            Assert.Equal((ulong)Layout.Length + ExpectedDirs, tree.GetNodeCount());
            var rootInfo = tree.GetNodeInfo(tree.GetRootId());
            Assert.Equal(NodeKind.Directory, rootInfo.Kind);
            Assert.Equal((ulong)Layout.Length, rootInfo.FileCount);
            Assert.Equal(summary.TotalSize, rootInfo.Size);
            Assert.Equal(4u, tree.GetChildren(tree.GetRootId(), 0, 100).TotalChildren); // docs, media, web, engine

            // 2. Treemap layout at 1280x720, depth 3
            const float w = 1280f, h = 720f;
            var rects = tree.ComputeLayout(tree.GetRootId(), w, h, maxDepth: 3, minSizePx: 2.0f);
            Assert.NotEmpty(rects);
            foreach (var r in rects)
            {
                Assert.True(r.X >= 0f && r.Y >= 0f, $"Rect origin ({r.X},{r.Y}) outside viewport");
                Assert.True(r.X + r.Width <= w + 0.5f, $"Rect right edge {r.X + r.Width} > {w}");
                Assert.True(r.Y + r.Height <= h + 0.5f, $"Rect bottom edge {r.Y + r.Height} > {h}");
                Assert.True(r.Width > 0f && r.Height > 0f);
            }
            // clip.mp4 sits at depth 2, so it must have its own tile.
            Assert.Contains(rects, r => tree.GetNodeInfo(r.NodeId).Name == "clip.mp4");

            // 3. Search with ext: and size: filters
            var rlib = await tree.SearchAsync("ext:rlib");
            Assert.Single(rlib);
            Assert.Equal("libengine.rlib", rlib[0].Name);
            Assert.True(IsUnder(rlib[0].FullPath, root.FullName), $"Search hit outside temp root: {rlib[0].FullPath}");

            var big = await tree.SearchAsync("size:>40KB kind:file");
            var bigNames = big.Select(b => b.Name).ToHashSet();
            Assert.Equal(new HashSet<string> { "engine.exe", "clip.mp4", "libengine.rlib" }, bigNames);
        }
        finally
        {
            try { root.Delete(recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task EndToEnd_DevStorageAndCleanupDryRun_FindsArtifactsAndLeavesFilesOnDisk()
    {
        var root = CreateFixture();
        try
        {
            using var scan = await ScanService.ScanDriveWithResultAsync(root.FullName);
            using var tree = await StorageTreeService.BuildAsync(scan);
            var before = Snapshot(root);
            Assert.Equal(Layout.Length, before.Count);

            // 4. Developer storage detection
            var artifacts = await DevStorageService.DiscoverArtifactsAsync(tree);
            var nodeModules = artifacts.SingleOrDefault(a => a.Name == "node_modules" && IsUnder(a.Path, root.FullName));
            var target = artifacts.SingleOrDefault(a => a.Name == "target" && IsUnder(a.Path, root.FullName));
            Assert.NotNull(nodeModules);
            Assert.NotNull(target);
            Assert.Equal(DevEcosystem.NodeJs, nodeModules.Ecosystem);
            Assert.Equal(DevEcosystem.Rust, target.Ecosystem);
            Assert.Equal("web", nodeModules.ProjectName);
            Assert.Equal("engine", target.ProjectName);
            Assert.True(nodeModules.SizeBytes >= 32768 + 12288);
            Assert.True(target.SizeBytes >= 131072 + 49152);

            // Tree-derived project artifacts must all come from the scanned folder.
            foreach (var a in artifacts.Where(a => a.Kind == DevArtifactKind.ProjectArtifact))
            {
                Assert.True(IsUnder(a.Path, root.FullName), $"Project artifact outside scanned root: {a.Path}");
            }

            // 5. Unified cleanup analysis (what the Cleanup and Overview pages show)
            var set = await ReclaimableService.LoadAsync(tree);
            var allowedRoots = new List<string> { root.FullName };
            foreach (var v in new[] { "USERPROFILE", "LOCALAPPDATA", "APPDATA", "ProgramData" })
            {
                var p = Environment.GetEnvironmentVariable(v);
                if (!string.IsNullOrEmpty(p)) allowedRoots.Add(p);
            }

            foreach (var c in set.Candidates.Where(c => c.Path != null))
            {
                Assert.True(allowedRoots.Any(r => IsUnder(c.Path!, r)), $"Candidate outside allowed roots: {c.Path}");
                Assert.False(allowedRoots.Skip(1).Any(r => string.Equals(
                    Path.GetFullPath(c.Path!).TrimEnd('\\'), Path.GetFullPath(r).TrimEnd('\\'), StringComparison.OrdinalIgnoreCase)),
                    $"Candidate is an allowed root itself: {c.Path}");
                if (!c.IsProtected)
                {
                    Assert.False(CleanupService.IsPathProtected(c.Path!), $"Unflagged protected path: {c.Path}");
                }
            }

            // 6. Dry run ONLY, and only for the artifacts inside our temp folder: the Developer page's
            //    direct clean and the unified Cleanup page's executor.
            foreach (var a in new[] { nodeModules, target })
            {
                var r = await DevStorageService.CleanArtifactAsync(a.Path, dryRun: true, sendToRecycleBin: true);
                Assert.True(r.IsDryRun);
                Assert.Equal(0UL, r.FilesFailed);
                Assert.True(r.FilesReclaimed >= 1);
            }

            var mine = new[] { nodeModules, target }.Select(a => new UnifiedCandidate
            {
                Source = CandidateSource.DevArtifact,
                Name = a.Name,
                Path = a.Path,
                Bytes = a.SizeBytes,
                FileCount = a.FileCount,
                Risk = a.RiskLevel,
                IsProtected = CleanupService.IsPathProtected(a.Path),
                IsSelected = true,
            }).ToList();
            Assert.All(mine, c => Assert.False(c.IsProtected));
            Assert.All(mine, c => Assert.True(IsUnder(c.Path!, root.FullName)));

            var report = await ReclaimableService.ExecuteAsync(tree, mine, dryRun: true, sendToRecycleBin: true);
            Assert.Equal(0, report.ItemsFailed);
            Assert.Equal(0UL, report.FilesFailed);
            Assert.True(report.FilesReclaimed >= 2, $"Dry run should count both artifacts, got {report.FilesReclaimed}");

            // Every file is still on disk with its original size.
            var after = Snapshot(root);
            Assert.Equal(before, after);
            Assert.True(Directory.Exists(nodeModules.Path));
            Assert.True(Directory.Exists(target.Path));
        }
        finally
        {
            try { root.Delete(recursive: true); } catch { }
        }
    }
}
