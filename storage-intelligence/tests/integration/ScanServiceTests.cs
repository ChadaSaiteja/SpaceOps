using StorageIntelligence.Native;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Real C# -> P/Invoke -> Rust -> callback -> C# round trip (Sub-phase 2.8 acceptance
/// criteria), exercised against an actual temp directory rather than mocks.
/// </summary>
public class ScanServiceTests
{
    [Fact]
    public async Task ScanDriveAsync_ScansRealDirectory_ReturnsAccurateSummary()
    {
        var dir = Directory.CreateTempSubdirectory("si_integration_test_");
        try
        {
            File.WriteAllText(Path.Combine(dir.FullName, "a.txt"), "hello");
            File.WriteAllText(Path.Combine(dir.FullName, "b.txt"), "world!");
            Directory.CreateDirectory(Path.Combine(dir.FullName, "sub"));
            File.WriteAllText(Path.Combine(dir.FullName, "sub", "c.txt"), "12345");

            var progressReports = new List<ScanProgressInfo>();
            var progress = new Progress<ScanProgressInfo>(p => progressReports.Add(p));

            var summary = await ScanService.ScanDriveAsync(dir.FullName, CancellationToken.None, progress);

            Assert.Equal(3ul, summary.TotalFiles);
            Assert.Equal(2ul, summary.TotalDirs); // root + sub
            Assert.Equal(0ul, summary.InaccessibleCount);
            Assert.True(summary.TotalSize >= 16);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task ScanDriveAsync_InvalidPath_ThrowsScanException()
    {
        var missing = Path.Combine(Path.GetTempPath(), "si_integration_missing_" + Guid.NewGuid());

        var ex = await Assert.ThrowsAsync<ScanException>(
            () => ScanService.ScanDriveAsync(missing));

        Assert.Equal(1, ex.ErrorCode); // ScanErrorCode.InvalidPath
    }

    [Fact]
    public async Task ScanDriveAsync_Cancelled_ThrowsScanExceptionWithCancelledCode()
    {
        var dir = Directory.CreateTempSubdirectory("si_integration_cancel_");
        try
        {
            File.WriteAllText(Path.Combine(dir.FullName, "f.txt"), "data");

            using var cts = new CancellationTokenSource();
            cts.Cancel(); // pre-cancelled: .NET fires Register()'s callback synchronously,
                          // so native code observes cancellation before scanning starts.

            var ex = await Assert.ThrowsAsync<ScanException>(
                () => ScanService.ScanDriveAsync(dir.FullName, cts.Token));

            Assert.Equal(2, ex.ErrorCode); // ScanErrorCode.Cancelled
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task ScanDriveAsync_HighThroughputKernelBuffered_MatchesClusterAllocation()
    {
        var dir = Directory.CreateTempSubdirectory("si_kernel_buffered_test_");
        try
        {
            for (int i = 0; i < 20; i++)
            {
                File.WriteAllText(Path.Combine(dir.FullName, $"file_{i}.txt"), $"content_{i}");
            }
            var sub1 = Directory.CreateDirectory(Path.Combine(dir.FullName, "nested_dir"));
            for (int i = 0; i < 10; i++)
            {
                File.WriteAllText(Path.Combine(sub1.FullName, $"sub_file_{i}.txt"), $"sub_content_{i}");
            }

            var summary = await ScanService.ScanDriveAsync(dir.FullName, CancellationToken.None);

            Assert.Equal(30ul, summary.TotalFiles);
            Assert.Equal(2ul, summary.TotalDirs); // root + nested_dir
            Assert.Equal(0ul, summary.InaccessibleCount);
            // On Windows NTFS, 30 files are allocated in multiples of 4096 bytes: at least 30 * 4096 = 122,880 bytes
            Assert.True(summary.TotalSize >= 122880);
            Assert.Equal(0ul, summary.TotalSize % 4096);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
