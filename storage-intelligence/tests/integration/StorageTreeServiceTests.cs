using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Real C# -> P/Invoke -> Rust StorageTree round trip (Sub-phase 3.9 acceptance criteria),
/// exercised against an actual directory structure on disk.
/// </summary>
public class StorageTreeServiceTests
{
    [Fact]
    public async Task BuildAsync_FromRealScan_EnablesFullTreeNavigationAndQueries()
    {
        var dir = Directory.CreateTempSubdirectory("si_tree_test_");
        try
        {
            // Set up test hierarchy:
            // dir/
            //   file1.txt (1000 bytes)
            //   file2.log (5000 bytes)
            //   sub/
            //     file3.bin (10000 bytes)
            //     file4.dat (2000 bytes)
            //   empty_sub/
            File.WriteAllBytes(Path.Combine(dir.FullName, "file1.txt"), new byte[1000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "file2.log"), new byte[10000]);

            var subDir = Directory.CreateDirectory(Path.Combine(dir.FullName, "sub"));
            File.WriteAllBytes(Path.Combine(subDir.FullName, "file3.bin"), new byte[20000]);
            File.WriteAllBytes(Path.Combine(subDir.FullName, "file4.dat"), new byte[6000]);

            Directory.CreateDirectory(Path.Combine(dir.FullName, "empty_sub"));

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            Assert.False(scanResult.IsInvalid);

            using var tree = await StorageTreeService.BuildAsync(scanResult);
            Assert.False(tree.Handle.IsInvalid);

            // 1. Root & node count
            ulong rootId = tree.GetRootId();
            ulong nodeCount = tree.GetNodeCount();
            Assert.True(nodeCount >= 6, $"Expected >= 6 nodes, got {nodeCount}");

            // 2. Root node info
            var rootInfo = tree.GetNodeInfo(rootId, includeFullPath: true);
            Assert.Equal(rootId, rootInfo.Id);
            Assert.Null(rootInfo.ParentId);
            Assert.Equal(NodeKind.Directory, rootInfo.Kind);
            Assert.Equal(4ul, rootInfo.FileCount);
            Assert.Equal(2ul, rootInfo.DirCount); // sub and empty_sub
            Assert.True(rootInfo.Size >= 18000, $"Expected >= 18000 bytes, got {rootInfo.Size}");
            Assert.False(string.IsNullOrEmpty(rootInfo.Name));
            Assert.False(string.IsNullOrEmpty(rootInfo.FullPath));

            // 3. Children of root (sorted by size desc)
            var children = tree.GetChildren(rootId, offset: 0, limit: 10);
            Assert.Equal(children.TotalChildren, (uint)children.NodeIds.Length);
            Assert.True(children.TotalChildren >= 3); // sub, file2, file1, empty_sub

            ulong prevSize = ulong.MaxValue;
            foreach (var childId in children.NodeIds)
            {
                var childInfo = tree.GetNodeInfo(childId);
                Assert.True(childInfo.Size <= prevSize, "Children must be sorted descending by size");
                Assert.Equal(rootId, childInfo.ParentId);
                prevSize = childInfo.Size;
            }

            // 4. Pagination
            var page1 = tree.GetChildren(rootId, offset: 0, limit: 2);
            Assert.Equal(2, page1.NodeIds.Length);
            var page2 = tree.GetChildren(rootId, offset: 2, limit: 2);
            if (page1.TotalChildren > 2)
            {
                Assert.True(page2.NodeIds.Length > 0);
                Assert.NotEqual(page1.NodeIds[0], page2.NodeIds[0]);
            }

            // 5. Ancestors
            // Find a child in sub/
            ulong subDirId = ulong.MaxValue;
            for (uint i = 0; i < children.TotalChildren; i++)
            {
                var id = children.NodeIds[i];
                var info = tree.GetNodeInfo(id);
                if (info.Kind == NodeKind.Directory && info.Name == "sub")
                {
                    subDirId = id;
                    break;
                }
            }
            Assert.NotEqual(ulong.MaxValue, subDirId);

            var subChildren = tree.GetChildren(subDirId, offset: 0, limit: 10);
            Assert.Equal(2u, subChildren.TotalChildren);
            ulong file3Id = subChildren.NodeIds[0]; // file3.bin (10000 bytes) should be first

            var ancestors = tree.GetAncestors(file3Id);
            Assert.Equal(3, ancestors.Length); // file3 -> sub -> root
            Assert.Equal(file3Id, ancestors[0]);
            Assert.Equal(subDirId, ancestors[1]);
            Assert.Equal(rootId, ancestors[2]);

            // 6. Top files by size
            var topFiles = tree.GetTopFilesBySize(rootId, limit: 3);
            Assert.Equal(3, topFiles.Length);
            var top1 = tree.GetNodeInfo(topFiles[0]);
            var top2 = tree.GetNodeInfo(topFiles[1]);
            var top3 = tree.GetNodeInfo(topFiles[2]);

            Assert.Equal("file3.bin", top1.Name);
            Assert.True(top1.Size >= 10000ul);
            Assert.Equal("file2.log", top2.Name);
            Assert.True(top2.Size >= 5000ul);
            Assert.Equal("file4.dat", top3.Name);
            Assert.True(top3.Size >= 2000ul);
            Assert.True(top1.Size > top2.Size);
            Assert.True(top2.Size > top3.Size);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Build_TwiceOnSameScanResult_ThrowsScanException()
    {
        var dir = Directory.CreateTempSubdirectory("si_tree_twice_");
        try
        {
            File.WriteAllText(Path.Combine(dir.FullName, "a.txt"), "hello");
            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);

            // First build consumes the events
            using var tree1 = StorageTreeService.Build(scanResult);
            Assert.False(tree1.Handle.IsInvalid);

            // Second build on same scanResult must fail (events were moved)
            var ex = Assert.Throws<ScanException>(() => StorageTreeService.Build(scanResult));
            Assert.Contains("events already consumed", ex.Message);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task DisposedTree_ThrowsObjectDisposedException()
    {
        var dir = Directory.CreateTempSubdirectory("si_tree_disposed_");
        try
        {
            File.WriteAllText(Path.Combine(dir.FullName, "test.txt"), "hello");
            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            var tree = StorageTreeService.Build(scanResult);
            tree.Dispose();

            Assert.Throws<ObjectDisposedException>(() => tree.GetRootId());
            Assert.Throws<ObjectDisposedException>(() => tree.GetNodeCount());
            Assert.Throws<ObjectDisposedException>(() => tree.GetNodeInfo(0));
            Assert.Throws<ObjectDisposedException>(() => tree.GetChildren(0, 0, 10));
            Assert.Throws<ObjectDisposedException>(() => tree.GetAncestors(0));
            Assert.Throws<ObjectDisposedException>(() => tree.GetTopFilesBySize(0, 5));
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
