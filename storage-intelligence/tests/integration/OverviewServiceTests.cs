using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using StorageIntelligence.Services;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>Overview numbers must match the scanned tree exactly.</summary>
public class OverviewServiceTests
{
    [Fact]
    public async Task Compute_SlicesAddUpToTreeTotal_AndTopConsumersAreSorted()
    {
        var dir = Directory.CreateTempSubdirectory("si_overview_test_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "movie.mp4"), new byte[200_000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "backup.zip"), new byte[80_000]);
            var sub = Directory.CreateDirectory(Path.Combine(dir.FullName, "docs"));
            File.WriteAllBytes(Path.Combine(sub.FullName, "report.pdf"), new byte[30_000]);
            File.WriteAllBytes(Path.Combine(sub.FullName, "notes.xyz"), new byte[10_000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var snap = await OverviewService.ComputeAsync(tree);
            var root = tree.GetNodeInfo(tree.GetRootId());

            Assert.Equal(root.Size, snap.TotalBytes);
            Assert.Equal(root.Size, (ulong)snap.Slices.Sum(s => (long)s.Bytes));

            var video = snap.Slices.Single(s => s.Category == FileCategory.Video);
            Assert.True(video.Bytes >= 200_000);
            Assert.Contains(snap.Slices, s => s.Category == FileCategory.Archive);
            Assert.Contains(snap.Slices, s => s.Category == FileCategory.Document);
            Assert.Contains(snap.Slices, s => s.Category == FileCategory.Other);

            Assert.InRange(snap.TopConsumers.Count, 1, OverviewService.TopConsumerCount);
            Assert.Equal("movie.mp4", snap.TopConsumers[0].Name);
            for (int i = 1; i < snap.TopConsumers.Count; i++)
            {
                Assert.True(snap.TopConsumers[i].Bytes <= snap.TopConsumers[i - 1].Bytes);
            }
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
