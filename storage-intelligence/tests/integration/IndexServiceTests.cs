using System;
using System.IO;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Integration tests verifying Incremental Indexing, SQLite Persistence & Synchronization (ADR-016, PRD §10, §22).
/// </summary>
public class IndexServiceTests
{
    [Fact]
    public async Task SaveAndLoadTree_Roundtrip_PersistsAndHydratesAccurately()
    {
        var tempDir = Directory.CreateTempSubdirectory("si_index_test_");
        var dbFile = Path.Combine(tempDir.FullName, "test_index.db");

        try
        {
            var subDir = Directory.CreateDirectory(Path.Combine(tempDir.FullName, "subfolder"));
            File.WriteAllBytes(Path.Combine(subDir.FullName, "file1.bin"), new byte[12000]);
            File.WriteAllBytes(Path.Combine(subDir.FullName, "file2.bin"), new byte[24000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(tempDir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            ulong originalCount = tree.GetNodeCount();
            var originalRoot = tree.GetNodeInfo(tree.GetRootId());

            // 1. Save to SQLite index
            IndexService.SaveTree(tree, tempDir.FullName, dbFile);
            Assert.True(File.Exists(dbFile), "Database file must be created on disk");

            // 2. Load from SQLite index
            using var loadedTree = IndexService.LoadTree(tempDir.FullName, dbFile);
            Assert.NotNull(loadedTree);

            ulong loadedCount = loadedTree.GetNodeCount();
            var loadedRoot = loadedTree.GetNodeInfo(loadedTree.GetRootId());

            Assert.Equal(originalCount, loadedCount);
            Assert.Equal(originalRoot.Size, loadedRoot.Size);
            Assert.Equal(originalRoot.FileCount, loadedRoot.FileCount);
        }
        finally
        {
            try { tempDir.Delete(recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task GetStats_ReturnsAccurateVolumeMetrics()
    {
        var tempDir = Directory.CreateTempSubdirectory("si_stats_test_");
        var dbFile = Path.Combine(tempDir.FullName, "stats.db");

        try
        {
            File.WriteAllBytes(Path.Combine(tempDir.FullName, "sample.dat"), new byte[8192]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(tempDir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            IndexService.SaveTree(tree, tempDir.FullName, dbFile);

            var stats = IndexService.GetStats(tempDir.FullName, dbFile);
            Assert.NotNull(stats);
            Assert.Equal(tempDir.FullName, stats.DriveOrPath);
            Assert.Equal(tree.GetNodeCount(), stats.NodeCount);
            Assert.True(stats.TotalSizeBytes >= 8192);
            Assert.True(stats.LastScanTimeUnixSecs > 0);
        }
        finally
        {
            try { tempDir.Delete(recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task DeleteVolume_CascadesAndRemovesIndexedRecords()
    {
        var tempDir = Directory.CreateTempSubdirectory("si_del_test_");
        var dbFile = Path.Combine(tempDir.FullName, "delete.db");

        try
        {
            File.WriteAllBytes(Path.Combine(tempDir.FullName, "test.dat"), new byte[4096]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(tempDir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            IndexService.SaveTree(tree, tempDir.FullName, dbFile);
            Assert.NotNull(IndexService.GetStats(tempDir.FullName, dbFile));

            bool deleted = IndexService.DeleteVolume(tempDir.FullName, dbFile);
            Assert.True(deleted, "DeleteVolume must return true for existing volume");

            var statsAfter = IndexService.GetStats(tempDir.FullName, dbFile);
            Assert.Null(statsAfter);
        }
        finally
        {
            try { tempDir.Delete(recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task SyncTree_IncrementallyCapturesFileModifications()
    {
        var tempDir = Directory.CreateTempSubdirectory("si_sync_test_");
        var dbFile = Path.Combine(tempDir.FullName, "sync.db");

        try
        {
            var testFile = Path.Combine(tempDir.FullName, "dynamic.txt");
            File.WriteAllBytes(testFile, new byte[1000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(tempDir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            IndexService.SaveTree(tree, tempDir.FullName, dbFile);

            // Modify file on disk with more content (crossing cluster boundary)
            await Task.Delay(50);
            File.WriteAllBytes(testFile, new byte[16000]);

            var report = IndexService.SyncTree(tree, tempDir.FullName, dbFile);

            Assert.NotNull(report);
            Assert.True(report.NodesUpdated >= 1, "At least one node must be updated");
            Assert.True(report.BytesDelta > 0, "Bytes delta must be positive after appending bytes");

            // Verify in-memory tree size updated
            var rootInfo = tree.GetNodeInfo(tree.GetRootId());
            Assert.True(rootInfo.Size >= (ulong)File.ReadAllBytes(testFile).Length);
        }
        finally
        {
            try { tempDir.Delete(recursive: true); } catch { }
        }
    }
}
