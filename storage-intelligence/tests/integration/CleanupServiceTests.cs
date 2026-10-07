using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Integration tests verifying the Cleanup Engine and Zero-Accident Safety Model (ADR-013, PRD §12).
/// </summary>
public class CleanupServiceTests
{
    [Fact]
    public async Task DetectCandidates_ReturnsAllStandardRules_WithExplanations()
    {
        var dir = Directory.CreateTempSubdirectory("si_cleanup_detect_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "sample.txt"), new byte[1024]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var candidates = await tree.DetectCleanupCandidatesAsync();

            Assert.NotEmpty(candidates);
            Assert.True(candidates.Length >= 7, $"Expected at least 7 standard rules, got {candidates.Length}");

            // Verify every candidate satisfies PRD §12 explanation contract
            foreach (var cand in candidates)
            {
                Assert.False(string.IsNullOrWhiteSpace(cand.Name), "Name must not be empty");
                Assert.False(string.IsNullOrWhiteSpace(cand.Description), "Description must not be empty");
                Assert.False(string.IsNullOrWhiteSpace(cand.PathDisplay), "PathDisplay must not be empty");
                Assert.False(string.IsNullOrWhiteSpace(cand.Reason), "Reason must not be empty");
                Assert.False(string.IsNullOrWhiteSpace(cand.Consequence), "Consequence must not be empty");
            }

            // Verify standard rules are present
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.UserTemp);
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.SystemTemp);
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.WindowsUpdate);
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.CrashDumps);
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.Thumbcache);
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.RecycleBin);
            Assert.Contains(candidates, c => c.RuleId == CleanupRuleId.StaleLogs);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public void IsPathProtected_GuardsSystemRoots()
    {
        // Core Windows system directories MUST be protected
        Assert.True(CleanupService.IsPathProtected("C:\\"));
        Assert.True(CleanupService.IsPathProtected("C:\\Windows"));
        Assert.True(CleanupService.IsPathProtected("C:\\Windows\\System32"));
        Assert.True(CleanupService.IsPathProtected("C:\\Windows\\System32\\kernel32.dll"));
        Assert.True(CleanupService.IsPathProtected("C:\\Windows\\WinSxS"));
        Assert.True(CleanupService.IsPathProtected("C:\\Program Files"));
        Assert.True(CleanupService.IsPathProtected("C:\\Program Files (x86)"));

        // User profile root & libraries MUST be protected
        string userProfile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (!string.IsNullOrEmpty(userProfile))
        {
            Assert.True(CleanupService.IsPathProtected(userProfile));
            Assert.True(CleanupService.IsPathProtected(Path.Combine(userProfile, "Desktop")));
            Assert.True(CleanupService.IsPathProtected(Path.Combine(userProfile, "Documents")));
        }

        // Safe targets must NOT be flagged as protected
        Assert.False(CleanupService.IsPathProtected("C:\\Windows\\Temp"));
        Assert.False(CleanupService.IsPathProtected("C:\\Windows\\SoftwareDistribution\\Download"));
    }

    [Fact]
    public async Task ExecuteRule_DryRun_AccuratelyReportsBytesWithoutDeleting()
    {
        var dir = Directory.CreateTempSubdirectory("si_cleanup_dryrun_");
        try
        {
            // Create a fake crash dump in the scanned directory
            string dumpFile = Path.Combine(dir.FullName, "app_crash.dmp");
            File.WriteAllBytes(dumpFile, new byte[16384]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var report = await tree.ExecuteCleanupRuleAsync(CleanupRuleId.CrashDumps, dryRun: true);

            Assert.True(report.IsDryRun);
            Assert.True(report.FilesReclaimed >= 1);
            Assert.True(report.BytesReclaimed >= 16384);
            Assert.Equal(0u, report.FilesFailed);

            // Invariant 3: Dry run MUST NOT delete the file!
            Assert.True(File.Exists(dumpFile), "File must still exist after dry-run simulation");
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
