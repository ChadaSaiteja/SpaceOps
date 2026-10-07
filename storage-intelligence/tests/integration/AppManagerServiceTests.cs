using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Integration tests verifying Application Management and Storage Attribution (ADR-014, PRD §14).
/// </summary>
public class AppManagerServiceTests
{
    [Fact]
    public void DiscoverApplications_FindsInstalledApps()
    {
        var apps = AppManagerService.DiscoverApplications();

        Assert.NotNull(apps);
        Assert.NotEmpty(apps);

        // Every discovered app must have an ID and non-empty Name
        foreach (var app in apps)
        {
            Assert.False(string.IsNullOrWhiteSpace(app.Id), "App ID must not be blank");
            Assert.False(string.IsNullOrWhiteSpace(app.Name), "App Name must not be blank");
            Assert.True(app.Kind == AppKind.Win32 || app.Kind == AppKind.Msix);
        }

        // Verify sorting: display size should be in descending order
        for (int i = 0; i < apps.Count - 1; i++)
        {
            Assert.True(
                apps[i].DisplaySizeBytes >= apps[i + 1].DisplaySizeBytes,
                $"Apps must be sorted by size descending: {apps[i].Name} ({apps[i].DisplaySizeBytes}) vs {apps[i + 1].Name} ({apps[i + 1].DisplaySizeBytes})");
        }
    }

    [Fact]
    public async Task DiscoverApplications_AttributesSizeFromTree()
    {
        var dir = Directory.CreateTempSubdirectory("si_app_test_");
        try
        {
            var appDir = Directory.CreateDirectory(Path.Combine(dir.FullName, "TestApp"));
            File.WriteAllBytes(Path.Combine(appDir.FullName, "binary.exe"), new byte[50000]);
            File.WriteAllBytes(Path.Combine(appDir.FullName, "data.bin"), new byte[100000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            // Test FindByPath on tree
            var node = tree.FindByPath(appDir.FullName);
            Assert.NotNull(node);
            Assert.Equal("TestApp", node.Value.Name);
            Assert.True(node.Value.Size >= 150000);
            Assert.Equal(2UL, node.Value.FileCount);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public void DiscoverLeftovers_ReturnsValidEntries()
    {
        var apps = AppManagerService.DiscoverApplications();
        var leftovers = AppManagerService.DiscoverLeftovers(apps);

        Assert.NotNull(leftovers);
        // Leftovers may be empty or contain orphaned folders
        foreach (var leftover in leftovers)
        {
            Assert.False(string.IsNullOrWhiteSpace(leftover.Id));
            Assert.False(string.IsNullOrWhiteSpace(leftover.AppName));
            Assert.False(string.IsNullOrWhiteSpace(leftover.Path));
            Assert.True(leftover.SizeBytes > 0 || leftover.FileCount > 0);
        }
    }

    [Fact]
    public void InstalledAppInfo_FormattingAndHelpers()
    {
        var app = new InstalledAppInfo(
            Id: "TestApp1",
            Name: "Test Application",
            Version: "1.0.0",
            Publisher: "Test Publisher",
            InstallLocation: @"C:\Program Files\TestApp",
            UninstallString: @"C:\Program Files\TestApp\uninstall.exe",
            QuietUninstallString: "",
            InstallDate: "20260101",
            Kind: AppKind.Win32,
            EstimatedSizeBytes: 10485760, // 10 MB
            ActualSizeBytes: 20971520,    // 20 MB
            FileCount: 42,
            IsSystemComponent: false
        );

        Assert.Equal("Desktop App", app.KindText);
        Assert.Equal(20971520UL, app.DisplaySizeBytes);
        Assert.Contains("20 MB", app.FormattedSize);
        Assert.Equal("20 MB", InstalledAppInfo.FormatBytes(20971520UL));
        Assert.Equal("1.5 GB", InstalledAppInfo.FormatBytes(1610612736UL));
    }
}
