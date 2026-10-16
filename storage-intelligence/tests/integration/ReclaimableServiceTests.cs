using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using StorageIntelligence.Services;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>Safety behavior of the unified cleanup list.</summary>
public class ReclaimableServiceTests
{
    [Fact]
    public async Task Execute_DryRunLeftover_CountsBytesButKeepsFolder()
    {
        var dir = Directory.CreateTempSubdirectory("si_reclaim_dry_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "a.bin"), new byte[5000]);
            using var scan = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scan);

            var candidate = new UnifiedCandidate
            {
                Source = CandidateSource.AppLeftover,
                Name = "test leftover",
                Path = dir.FullName,
                Bytes = 5000,
                FileCount = 1,
                IsSelected = true,
            };

            var report = await ReclaimableService.ExecuteAsync(tree, new[] { candidate }, dryRun: true, sendToRecycleBin: true);

            Assert.Equal(5000UL, report.BytesReclaimed);
            Assert.Equal(0, report.ItemsFailed);
            Assert.True(Directory.Exists(dir.FullName));
            Assert.True(File.Exists(Path.Combine(dir.FullName, "a.bin")));
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Execute_ProtectedLeftoverPath_IsSkippedAndNeverDeleted()
    {
        var dir = Directory.CreateTempSubdirectory("si_reclaim_prot_");
        try
        {
            using var scan = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scan);

            string windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
            Assert.True(CleanupService.IsPathProtected(windows));

            // Even if a protected path slipped into the list unflagged, execution must refuse it.
            var candidate = new UnifiedCandidate
            {
                Source = CandidateSource.AppLeftover,
                Name = "bogus",
                Path = windows,
                Bytes = 1,
                IsSelected = true,
            };

            var report = await ReclaimableService.ExecuteAsync(tree, new[] { candidate }, dryRun: false, sendToRecycleBin: true);

            Assert.Equal(1, report.ItemsFailed);
            Assert.Equal(0UL, report.BytesReclaimed);
            Assert.True(Directory.Exists(windows));
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public void ReclaimableSet_TotalsExcludeProtectedCandidates()
    {
        var set = new ReclaimableSet(new List<UnifiedCandidate>
        {
            new() { Source = CandidateSource.SystemJunk, Bytes = 100 },
            new() { Source = CandidateSource.DevArtifact, Bytes = 40 },
            new() { Source = CandidateSource.AppLeftover, Bytes = 7, IsProtected = true },
        }, new List<string>());

        Assert.Equal(140UL, set.TotalBytes);
        Assert.Equal(0UL, set.BytesFor(CandidateSource.AppLeftover));
        Assert.Equal(1, set.CountFor(CandidateSource.DevArtifact));
    }
}
