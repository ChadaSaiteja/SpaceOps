using System;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using StorageIntelligence.Native;

namespace StorageIntelligence.Services;

/// <summary>
/// Application-wide state shared by the shell and every page: the current storage tree,
/// scan/sync orchestration, and the status bar text. Raises events on the UI thread
/// (all methods are expected to be called from the UI thread).
/// </summary>
public sealed class AppSession
{
    public static AppSession Current { get; } = new();

    private CancellationTokenSource? _scanCts;

    public StorageTreeService? Tree { get; private set; }
    public string TargetPath { get; set; } = string.Empty;
    public bool IsScanning { get; private set; }
    public bool IsSyncing { get; private set; }
    public string StatusText { get; private set; } = "Ready";
    public string MetricsText { get; private set; } = string.Empty;

    /// <summary>A different tree instance is now current (new scan or index load), or it was cleared.</summary>
    public event Action? TreeChanged;

    /// <summary>The current tree was mutated in place (incremental sync).</summary>
    public event Action? TreeUpdated;

    public event Action? ScanStateChanged;
    public event Action<string>? StatusChanged;
    public event Action<string>? MetricsChanged;

    /// <summary>Asks the shell to switch to a destination ("overview", "treemap", "cleanup", "developer", "applications").</summary>
    public event Action<string>? NavigateRequested;

    public void SetStatus(string text)
    {
        StatusText = text;
        StatusChanged?.Invoke(text);
    }

    public void SetMetrics(string text)
    {
        MetricsText = text;
        MetricsChanged?.Invoke(text);
    }

    public void RequestNavigation(string destination) => NavigateRequested?.Invoke(destination);

    private void ReplaceTree(StorageTreeService? newTree)
    {
        var old = Tree;
        Tree = newTree;
        TreeChanged?.Invoke();

        if (old != null && !ReferenceEquals(old, newTree))
        {
            // Background work (overview analysis, cleanup detection) may still hold the old handle for a moment.
            _ = DisposeLaterAsync(old);
        }
    }

    private static async Task DisposeLaterAsync(StorageTreeService tree)
    {
        await Task.Delay(TimeSpan.FromSeconds(3)).ConfigureAwait(false);
        tree.Dispose();
    }

    private void UpdateRootMetrics()
    {
        if (Tree == null)
        {
            SetMetrics(string.Empty);
            return;
        }
        var rootInfo = Tree.GetNodeInfo(Tree.GetRootId());
        SetMetrics($"Total Size: {Fmt.Bytes(rootInfo.Size)}");
    }

    public async Task ScanAsync()
    {
        string path = TargetPath.Trim();
        if (string.IsNullOrEmpty(path) || IsScanning) return;

        _scanCts = new CancellationTokenSource();
        IsScanning = true;
        ScanStateChanged?.Invoke();
        SetStatus($"Starting scan of {path}...");
        SetMetrics(string.Empty);

        var progress = new Progress<ScanProgressInfo>(p =>
        {
            SetStatus($"Scanning: {p.FilesScanned:N0} files  •  {Fmt.Bytes(p.BytesScanned)}");
        });

        try
        {
            var sw = Stopwatch.StartNew();
            using var scanResult = await ScanService.ScanDriveWithResultAsync(path, _scanCts.Token, progress);
            sw.Stop();

            SetStatus("Building in-memory storage tree...");

            var treeSw = Stopwatch.StartNew();
            var newTree = await StorageTreeService.BuildAsync(scanResult);
            treeSw.Stop();

            ReplaceTree(newTree);

            ulong nodeCount = newTree.GetNodeCount();
            SetStatus($"Ready • {nodeCount:N0} nodes scanned in {sw.Elapsed.TotalSeconds:F2}s (tree built in {treeSw.ElapsedMilliseconds}ms)");
            UpdateRootMetrics();

            // Asynchronously persist to SQLite index for instant future launches (ADR-016)
            _ = Task.Run(async () =>
            {
                try
                {
                    await IndexService.SaveTreeAsync(newTree, path);
                }
                catch
                {
                    // Non-fatal background indexing
                }
            });
        }
        catch (ScanException sex) when (sex.ErrorCode == 2)
        {
            SetStatus("Scan cancelled by user.");
        }
        catch (Exception ex)
        {
            SetStatus($"Error: {ex.Message}");
        }
        finally
        {
            IsScanning = false;
            _scanCts = null;
            ScanStateChanged?.Invoke();
        }
    }

    public void CancelScan()
    {
        _scanCts?.Cancel();
        SetStatus("Cancelling scan...");
    }

    /// <summary>Incremental sync of the current tree, or instant hydration from the SQLite index when nothing is loaded.</summary>
    public async Task SyncAsync()
    {
        string path = TargetPath.Trim();
        if (string.IsNullOrEmpty(path) || IsSyncing) return;

        IsSyncing = true;
        ScanStateChanged?.Invoke();

        try
        {
            if (Tree != null)
            {
                // Incremental synchronization via USN Journal or timestamp diff
                SetStatus($"Incrementally synchronizing {path} via USN / timestamp diff...");
                var report = await IndexService.SyncTreeAsync(Tree, path);

                TreeUpdated?.Invoke();
                SetStatus($"Synced in {report.SyncDurationMs}ms • {report.NodesUpdated} updated, {report.NodesRemoved} removed ({report.FormattedBytesDelta})");
                UpdateRootMetrics();
            }
            else
            {
                // Instant hydration from local SQLite index (< 100ms)
                SetStatus($"Checking SQLite index for {path}...");
                var cachedTree = await IndexService.LoadTreeAsync(path);

                if (cachedTree != null)
                {
                    ReplaceTree(cachedTree);

                    ulong nodeCount = cachedTree.GetNodeCount();
                    SetStatus($"Loaded from SQLite index in < 100ms ({nodeCount:N0} nodes)");
                    UpdateRootMetrics();
                }
                else
                {
                    SetStatus($"No cached index found for {path}. Click 'Scan' to perform the initial scan.");
                }
            }
        }
        catch (Exception ex)
        {
            SetStatus($"Index Sync Error: {ex.Message}");
        }
        finally
        {
            IsSyncing = false;
            ScanStateChanged?.Invoke();
        }
    }
}
