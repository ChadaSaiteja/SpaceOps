using System;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Integration tests verifying search and index engine across the FFI boundary (ADR-012).
/// </summary>
public class SearchServiceTests
{
    [Fact]
    public async Task Search_ByName_FindsExactAndSubstringMatches()
    {
        var dir = Directory.CreateTempSubdirectory("si_search_test_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "docker_image.vhdx"), new byte[50000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "docker_compose.yml"), new byte[2000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "notes.txt"), new byte[1000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var results = await tree.SearchAsync("docker");

            Assert.NotEmpty(results);
            Assert.Equal(2, results.Length);
            Assert.Contains(results, r => r.Name == "docker_image.vhdx");
            Assert.Contains(results, r => r.Name == "docker_compose.yml");
            Assert.DoesNotContain(results, r => r.Name == "notes.txt");
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Search_ByExtension_FiltersCorrectly()
    {
        var dir = Directory.CreateTempSubdirectory("si_search_ext_test_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "movie.mp4"), new byte[30000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "audio.mp3"), new byte[10000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "document.pdf"), new byte[5000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var results = await tree.SearchAsync("ext:mp4");

            Assert.Single(results);
            Assert.Equal("movie.mp4", results[0].Name);
            Assert.Equal(FileCategory.Video, results[0].Category);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Search_BySizeAndCategory_FiltersCorrectly()
    {
        var dir = Directory.CreateTempSubdirectory("si_search_cat_test_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "clip.mp4"), new byte[40000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "short.mp4"), new byte[2000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "backup.zip"), new byte[50000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            // Search for video files larger than 10KB
            var results = await tree.SearchAsync("type:video size:>10KB kind:file");

            Assert.Single(results);
            Assert.Equal("clip.mp4", results[0].Name);
            Assert.Equal(FileCategory.Video, results[0].Category);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Search_ByKindDirectory_ReturnsFoldersOnly()
    {
        var dir = Directory.CreateTempSubdirectory("si_search_dir_test_");
        try
        {
            var nodeModules = Directory.CreateDirectory(Path.Combine(dir.FullName, "node_modules"));
            File.WriteAllBytes(Path.Combine(nodeModules.FullName, "package.json"), new byte[1000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "node_modules.txt"), new byte[500]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var results = await tree.SearchAsync("node_modules kind:dir");

            Assert.Single(results);
            Assert.Equal("node_modules", results[0].Name);
            Assert.Equal(NodeKind.Directory, results[0].Kind);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Search_Relevance_RanksLargerFilesHigherWithinSameTier()
    {
        var dir = Directory.CreateTempSubdirectory("si_search_rank_test_");
        try
        {
            File.WriteAllBytes(Path.Combine(dir.FullName, "log_small.txt"), new byte[1000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "log_huge.txt"), new byte[100000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "log_medium.txt"), new byte[20000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var results = await tree.SearchAsync("log kind:file");

            Assert.Equal(3, results.Length);
            // Space-aware ranking: log_huge (100KB) > log_medium (20KB) > log_small (1KB)
            Assert.Equal("log_huge.txt", results[0].Name);
            Assert.Equal("log_medium.txt", results[1].Name);
            Assert.Equal("log_small.txt", results[2].Name);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task Search_SubMillisecondPerformance_OnInMemoryTree()
    {
        var dir = Directory.CreateTempSubdirectory("si_search_perf_test_");
        try
        {
            for (int i = 0; i < 50; i++)
            {
                File.WriteAllBytes(Path.Combine(dir.FullName, $"file_{i}.dat"), new byte[i * 1000 + 100]);
            }

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var sw = Stopwatch.StartNew();
            var results = await tree.SearchAsync("file size:>10KB", maxResults: 20);
            sw.Stop();

            Assert.NotEmpty(results);
            Assert.True(sw.ElapsedMilliseconds < 50, $"Search should be instant, took {sw.ElapsedMilliseconds} ms");
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
