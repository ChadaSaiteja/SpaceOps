using System;
using System.IO;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>
/// High-level metrics for an indexed volume stored in SQLite (ADR-016).
/// </summary>
public sealed record IndexStats(
    ulong VolumeId,
    string DriveOrPath,
    ulong NodeCount,
    ulong TotalSizeBytes,
    ulong LastScanTimeUnixSecs,
    ulong LastUsn
)
{
    public string FormattedSize => InstalledAppInfo.FormatBytes(TotalSizeBytes);
    public DateTime LastScanTime => DateTimeOffset.FromUnixTimeSeconds((long)LastScanTimeUnixSecs).LocalDateTime;
}

/// <summary>
/// Summary report returned upon incremental synchronization (ADR-016 §3).
/// </summary>
public sealed record IndexSyncReport(
    ulong NodesAdded,
    ulong NodesUpdated,
    ulong NodesRemoved,
    long BytesDelta,
    ulong SyncDurationMs
)
{
    public string FormattedBytesDelta => BytesDelta >= 0
        ? $"+{InstalledAppInfo.FormatBytes((ulong)BytesDelta)}"
        : $"-{InstalledAppInfo.FormatBytes((ulong)(-BytesDelta))}";
}

/// <summary>
/// Managed service providing SQLite persistence, sub-100ms launch hydration,
/// and incremental change synchronization via USN Journal and timestamp differentials (ADR-016).
/// </summary>
public static unsafe class IndexService
{
    /// <summary>
    /// Gets the canonical path to the application's local SQLite index database.
    /// </summary>
    public static string DefaultDatabasePath
    {
        get
        {
            string appData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            string dir = Path.Combine(appData, "StorageIntelligence");
            Directory.CreateDirectory(dir);
            return Path.Combine(dir, "storage_index.db");
        }
    }

    /// <summary>
    /// Persists an in-memory storage tree into the SQLite index database.
    /// </summary>
    public static void SaveTree(StorageTreeService tree, string rootPath, string? dbPath = null)
    {
        if (tree == null) throw new ArgumentNullException(nameof(tree));
        if (string.IsNullOrWhiteSpace(rootPath)) throw new ArgumentException("Root path cannot be empty", nameof(rootPath));

        string targetDb = string.IsNullOrWhiteSpace(dbPath) ? DefaultDatabasePath : dbPath;

        fixed (char* dbPtr = targetDb)
        fixed (char* rootPtr = rootPath)
        {
            int code = NativeMethods.index_save_tree(
                (ushort*)dbPtr,
                tree.Handle.DangerousHandle,
                (ushort*)rootPtr);

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to save storage tree to SQLite index at '{targetDb}'");
            }
        }
    }

    /// <summary>
    /// Asynchronously persists an in-memory storage tree on a background thread.
    /// </summary>
    public static Task SaveTreeAsync(StorageTreeService tree, string rootPath, string? dbPath = null)
    {
        return Task.Run(() => SaveTree(tree, rootPath, dbPath));
    }

    /// <summary>
    /// Hydrates an in-memory <see cref="StorageTreeService"/> directly from SQLite in sub-100ms.
    /// Returns null if no index exists for the specified path.
    /// </summary>
    public static StorageTreeService? LoadTree(string rootPath, string? dbPath = null)
    {
        if (string.IsNullOrWhiteSpace(rootPath)) throw new ArgumentException("Root path cannot be empty", nameof(rootPath));

        string targetDb = string.IsNullOrWhiteSpace(dbPath) ? DefaultDatabasePath : dbPath;
        if (!File.Exists(targetDb)) return null;

        TreeHandle* loaded = null;
        fixed (char* dbPtr = targetDb)
        fixed (char* rootPtr = rootPath)
        {
            int code = NativeMethods.index_load_tree(
                (ushort*)dbPtr,
                (ushort*)rootPtr,
                &loaded);

            if (code == 1) // InvalidPath / Not Found
            {
                return null;
            }

            if (code != 0 || loaded == null)
            {
                throw new ScanException(code, $"Failed to load storage tree from SQLite index at '{targetDb}'");
            }
        }

        return new StorageTreeService(new TreeSafeHandle((IntPtr)loaded));
    }

    /// <summary>
    /// Asynchronously hydrates a storage tree from SQLite on a background thread.
    /// </summary>
    public static Task<StorageTreeService?> LoadTreeAsync(string rootPath, string? dbPath = null)
    {
        return Task.Run(() => LoadTree(rootPath, dbPath));
    }

    /// <summary>
    /// Incrementally synchronizes filesystem changes into the tree and database.
    /// </summary>
    public static IndexSyncReport SyncTree(StorageTreeService tree, string rootPath, string? dbPath = null)
    {
        if (tree == null) throw new ArgumentNullException(nameof(tree));
        if (string.IsNullOrWhiteSpace(rootPath)) throw new ArgumentException("Root path cannot be empty", nameof(rootPath));

        string targetDb = string.IsNullOrWhiteSpace(dbPath) ? DefaultDatabasePath : dbPath;

        IndexSyncReportFfi report = default;
        fixed (char* dbPtr = targetDb)
        fixed (char* rootPtr = rootPath)
        {
            int code = NativeMethods.index_sync_tree(
                (ushort*)dbPtr,
                tree.Handle.DangerousHandle,
                (ushort*)rootPtr,
                &report);

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to incrementally synchronize tree with index at '{targetDb}'");
            }
        }

        return new IndexSyncReport(
            NodesAdded: report.nodes_added,
            NodesUpdated: report.nodes_updated,
            NodesRemoved: report.nodes_removed,
            BytesDelta: report.bytes_delta,
            SyncDurationMs: report.sync_duration_ms
        );
    }

    /// <summary>
    /// Asynchronously syncs changes into the tree on a background thread.
    /// </summary>
    public static Task<IndexSyncReport> SyncTreeAsync(StorageTreeService tree, string rootPath, string? dbPath = null)
    {
        return Task.Run(() => SyncTree(tree, rootPath, dbPath));
    }

    /// <summary>
    /// Retrieves indexed volume metrics for a path.
    /// </summary>
    public static IndexStats? GetStats(string rootPath, string? dbPath = null)
    {
        if (string.IsNullOrWhiteSpace(rootPath)) return null;

        string targetDb = string.IsNullOrWhiteSpace(dbPath) ? DefaultDatabasePath : dbPath;
        if (!File.Exists(targetDb)) return null;

        IndexStatsFfi stats = default;
        fixed (char* dbPtr = targetDb)
        fixed (char* rootPtr = rootPath)
        {
            int code = NativeMethods.index_get_stats(
                (ushort*)dbPtr,
                (ushort*)rootPtr,
                &stats);

            if (code == 1) // Not found
            {
                return null;
            }

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to query index stats from '{targetDb}'");
            }
        }

        return new IndexStats(
            VolumeId: stats.volume_id,
            DriveOrPath: rootPath,
            NodeCount: stats.node_count,
            TotalSizeBytes: stats.total_size,
            LastScanTimeUnixSecs: stats.last_scan_time,
            LastUsn: stats.last_usn
        );
    }

    /// <summary>
    /// Asynchronously retrieves index stats on a background thread.
    /// </summary>
    public static Task<IndexStats?> GetStatsAsync(string rootPath, string? dbPath = null)
    {
        return Task.Run(() => GetStats(rootPath, dbPath));
    }

    /// <summary>
    /// Deletes an indexed volume record and cascades all node records from the database.
    /// </summary>
    public static bool DeleteVolume(string rootPath, string? dbPath = null)
    {
        if (string.IsNullOrWhiteSpace(rootPath)) return false;

        string targetDb = string.IsNullOrWhiteSpace(dbPath) ? DefaultDatabasePath : dbPath;
        if (!File.Exists(targetDb)) return false;

        byte deleted = 0;
        fixed (char* dbPtr = targetDb)
        fixed (char* rootPtr = rootPath)
        {
            int code = NativeMethods.index_delete_volume(
                (ushort*)dbPtr,
                (ushort*)rootPtr,
                &deleted);

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to delete volume from index at '{targetDb}'");
            }
        }

        return deleted != 0;
    }

    /// <summary>
    /// Asynchronously deletes an indexed volume on a background thread.
    /// </summary>
    public static Task<bool> DeleteVolumeAsync(string rootPath, string? dbPath = null)
    {
        return Task.Run(() => DeleteVolume(rootPath, dbPath));
    }
}
