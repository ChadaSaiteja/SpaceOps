using System;
using System.IO;
using System.Threading.Tasks;
using StorageIntelligence.Native;
using Xunit;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Integration tests verifying treemap layout generation and hit-testing across the FFI boundary.
/// </summary>
public class TreemapServiceTests
{
    [Fact]
    public async Task ComputeLayout_FromRealScan_GeneratesValidTreemapGeometry()
    {
        var dir = Directory.CreateTempSubdirectory("si_treemap_test_");
        try
        {
            // Directory structure with distinct extensions
            File.WriteAllBytes(Path.Combine(dir.FullName, "movie.mp4"), new byte[20000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "song.mp3"), new byte[8000]);
            File.WriteAllBytes(Path.Combine(dir.FullName, "photo.jpg"), new byte[4000]);

            var subDir = Directory.CreateDirectory(Path.Combine(dir.FullName, "code_project"));
            File.WriteAllBytes(Path.Combine(subDir.FullName, "main.rs"), new byte[5000]);
            File.WriteAllBytes(Path.Combine(subDir.FullName, "App.cs"), new byte[3000]);

            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            ulong rootId = tree.GetRootId();

            float viewportW = 1200.0f;
            float viewportH = 800.0f;

            var rects = tree.ComputeLayout(rootId, viewportW, viewportH, maxDepth: 2, minSizePx: 2.0f);

            Assert.NotEmpty(rects);
            Assert.True(rects.Length >= 4, $"Expected >= 4 rects, got {rects.Length}");

            // Verify bounding coordinates
            foreach (var r in rects)
            {
                Assert.True(r.X >= 0.0f, $"X ({r.X}) should be >= 0");
                Assert.True(r.Y >= 0.0f, $"Y ({r.Y}) should be >= 0");
                Assert.True(r.X + r.Width <= viewportW + 0.5f, $"X+W ({r.X + r.Width}) exceeds viewport width {viewportW}");
                Assert.True(r.Y + r.Height <= viewportH + 0.5f, $"Y+H ({r.Y + r.Height}) exceeds viewport height {viewportH}");
                Assert.True(r.Width >= 2.0f);
                Assert.True(r.Height >= 2.0f);
            }

            // Verify largest file (movie.mp4) has category Video and largest area
            TreemapRect? movieRect = null;
            foreach (var r in rects)
            {
                var info = tree.GetNodeInfo(r.NodeId);
                if (info.Name == "movie.mp4")
                {
                    movieRect = r;
                    break;
                }
            }

            Assert.NotNull(movieRect);
            Assert.Equal(FileCategory.Video, movieRect.Value.Category);
            float movieArea = movieRect.Value.Width * movieRect.Value.Height;
            float totalArea = viewportW * viewportH;
            Assert.True(movieArea > totalArea * 0.35f, $"Movie area should be substantial, got {movieArea}/{totalArea}");

            // Hit test directly inside movie rect
            float centerX = movieRect.Value.X + movieRect.Value.Width / 2.0f;
            float centerY = movieRect.Value.Y + movieRect.Value.Height / 2.0f;

            var hit = TreemapService.FindNodeAtPoint(centerX, centerY, rects);
            Assert.NotNull(hit);
            Assert.Equal(movieRect.Value.NodeId, hit.Value.NodeId);

            // Hit test outside viewport bounds
            var hitOutside = TreemapService.FindNodeAtPoint(-10.0f, -10.0f, rects);
            Assert.Null(hitOutside);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }

    [Fact]
    public async Task ComputeLayout_ZeroViewport_ReturnsEmpty()
    {
        var dir = Directory.CreateTempSubdirectory("si_treemap_zero_");
        try
        {
            File.WriteAllText(Path.Combine(dir.FullName, "a.txt"), "hello");
            using var scanResult = await ScanService.ScanDriveWithResultAsync(dir.FullName);
            using var tree = await StorageTreeService.BuildAsync(scanResult);

            var rectsZeroW = tree.ComputeLayout(tree.GetRootId(), 0, 800);
            Assert.Empty(rectsZeroW);

            var rectsZeroH = tree.ComputeLayout(tree.GetRootId(), 800, 0);
            Assert.Empty(rectsZeroH);
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
