using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Integration tests verifying Developer Storage Intelligence & Safe Reclamation (ADR-015, PRD §13).
/// </summary>
public class DevStorageServiceTests
{
    [Fact]
    public void DiscoverArtifacts_GlobalCaches_ReturnsValidList()
    {
        var artifacts = DevStorageService.DiscoverArtifacts();

        Assert.NotNull(artifacts);
        // On developer machines, package caches (e.g. NuGet, Cargo, npm) will be present
        foreach (var art in artifacts)
        {
            Assert.False(string.IsNullOrWhiteSpace(art.Id), "Artifact ID must not be blank");
            Assert.False(string.IsNullOrWhiteSpace(art.Name), "Artifact Name must not be blank");
            Assert.False(string.IsNullOrWhiteSpace(art.Path), "Artifact Path must not be blank");
            Assert.False(string.IsNullOrWhiteSpace(art.Description), "Artifact Description must not be blank");
            Assert.False(string.IsNullOrWhiteSpace(art.CleanupCommand), "Artifact CleanupCommand must not be blank");
            Assert.True(art.Kind == DevArtifactKind.GlobalCache || art.Kind == DevArtifactKind.VirtualDisk || art.Kind == DevArtifactKind.ProjectArtifact);
        }

        // Verify descending size ordering
        for (int i = 0; i < artifacts.Count - 1; i++)
        {
            Assert.True(
                artifacts[i].SizeBytes >= artifacts[i + 1].SizeBytes,
                $"Artifacts must be sorted descending by size: {artifacts[i].Name} ({artifacts[i].SizeBytes}) vs {artifacts[i + 1].Name} ({artifacts[i + 1].SizeBytes})");
        }
    }

    [Fact]
    public async Task DiscoverArtifacts_WithTree_IdentifiesProjectArtifactsAndEcosystems()
    {
        var tempRoot = Directory.CreateTempSubdirectory("si_dev_test_");
        try
        {
            // 1. Node.js project: node_modules
            var nodeProj = Directory.CreateDirectory(Path.Combine(tempRoot.FullName, "WebFrontend", "node_modules", "react"));
            File.WriteAllBytes(Path.Combine(nodeProj.FullName, "index.js"), new byte[25000]);

            // 2. Rust project: target/
            var rustProj = Directory.CreateDirectory(Path.Combine(tempRoot.FullName, "CoreEngine", "target", "debug"));
            File.WriteAllBytes(Path.Combine(rustProj.FullName, "engine.rlib"), new byte[80000]);

            // 3. Python project: .venv
            var pyProj = Directory.CreateDirectory(Path.Combine(tempRoot.FullName, "DataService", ".venv", "Lib"));
            File.WriteAllBytes(Path.Combine(pyProj.FullName, "site.py"), new byte[15000]);

            // 4. .NET project: bin and obj
            var netProj = Directory.CreateDirectory(Path.Combine(tempRoot.FullName, "DesktopApp", "obj", "Debug"));
            File.WriteAllBytes(Path.Combine(netProj.FullName, "app.dll"), new byte[40000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(tempRoot.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var artifacts = DevStorageService.DiscoverArtifacts(tree);
            Assert.NotNull(artifacts);

            // Assert Node.js artifact
            var nodeArt = artifacts.FirstOrDefault(a => a.Name == "node_modules");
            Assert.NotNull(nodeArt);
            Assert.Equal(DevEcosystem.NodeJs, nodeArt.Ecosystem);
            Assert.Equal(DevArtifactKind.ProjectArtifact, nodeArt.Kind);
            Assert.Equal("WebFrontend", nodeArt.ProjectName);
            Assert.True(nodeArt.SizeBytes >= 25000);

            // Assert Rust artifact
            var rustArt = artifacts.FirstOrDefault(a => a.Name == "target");
            Assert.NotNull(rustArt);
            Assert.Equal(DevEcosystem.Rust, rustArt.Ecosystem);
            Assert.Equal(DevArtifactKind.ProjectArtifact, rustArt.Kind);
            Assert.Equal("CoreEngine", rustArt.ProjectName);
            Assert.True(rustArt.SizeBytes >= 80000);

            // Assert Python artifact
            var pyArt = artifacts.FirstOrDefault(a => a.Name == ".venv");
            Assert.NotNull(pyArt);
            Assert.Equal(DevEcosystem.Python, pyArt.Ecosystem);
            Assert.Equal(DevArtifactKind.ProjectArtifact, pyArt.Kind);
            Assert.Equal("DataService", pyArt.ProjectName);
            Assert.True(pyArt.SizeBytes >= 15000);

            // Assert .NET artifact
            var objArt = artifacts.FirstOrDefault(a => a.Name == "obj");
            Assert.NotNull(objArt);
            Assert.Equal(DevEcosystem.DotNet, objArt.Ecosystem);
            Assert.Equal(DevArtifactKind.ProjectArtifact, objArt.Kind);
            Assert.Equal("DesktopApp", objArt.ProjectName);
            Assert.True(objArt.SizeBytes >= 40000);
        }
        finally
        {
            try { tempRoot.Delete(recursive: true); } catch { }
        }
    }

    [Fact]
    public void CleanArtifact_DryRun_SimulatesWithoutDeleting()
    {
        var tempRoot = Directory.CreateTempSubdirectory("si_clean_dryrun_");
        try
        {
            var targetDir = Directory.CreateDirectory(Path.Combine(tempRoot.FullName, "target"));
            var testFile = Path.Combine(targetDir.FullName, "build.log");
            File.WriteAllText(testFile, "build output");

            var report = DevStorageService.CleanArtifact(targetDir.FullName, dryRun: true, sendToRecycleBin: true);

            Assert.True(report.IsDryRun);
            Assert.Equal(0UL, report.FilesFailed);
            Assert.True(report.FilesReclaimed >= 1);
            Assert.True(Directory.Exists(targetDir.FullName), "Directory must remain after dry run");
            Assert.True(File.Exists(testFile), "File must remain after dry run");
        }
        finally
        {
            try { tempRoot.Delete(recursive: true); } catch { }
        }
    }

    [Fact]
    public void CleanArtifact_StrictlyRejectsGitAndSystemPaths()
    {
        // 1. .git path must be strictly rejected
        var gitPath = @"C:\Projects\MyRepo\.git";
        var gitReport = DevStorageService.CleanArtifact(gitPath, dryRun: false, sendToRecycleBin: true);
        Assert.Equal(1UL, gitReport.FilesFailed);
        Assert.Equal(0UL, gitReport.FilesReclaimed);

        // 2. Source file path must be rejected
        var sourcePath = @"C:\Projects\MyRepo\Program.cs";
        var srcReport = DevStorageService.CleanArtifact(sourcePath, dryRun: false, sendToRecycleBin: true);
        Assert.Equal(1UL, srcReport.FilesFailed);
        Assert.Equal(0UL, srcReport.FilesReclaimed);

        // 3. Protected system folder must be rejected
        var sysPath = @"C:\Windows\System32";
        var sysReport = DevStorageService.CleanArtifact(sysPath, dryRun: false, sendToRecycleBin: true);
        Assert.Equal(1UL, sysReport.FilesFailed);
        Assert.Equal(0UL, sysReport.FilesReclaimed);
    }
}
