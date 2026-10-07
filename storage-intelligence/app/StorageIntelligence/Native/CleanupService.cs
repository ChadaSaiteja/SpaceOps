using System;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>
/// Unique identification of cleanup rule categories (PRD §12).
/// </summary>
public enum CleanupRuleId : uint
{
    UserTemp = 0,
    SystemTemp = 1,
    WindowsUpdate = 2,
    CrashDumps = 3,
    Thumbcache = 4,
    RecycleBin = 5,
    StaleLogs = 6,
}

/// <summary>
/// Safety classification risk levels (ADR-013).
/// </summary>
public enum RiskLevel : int
{
    Low = 0,
    Medium = 1,
    High = 2,
}

/// <summary>
/// Managed view of a detected cleanup candidate with full transparent explanations (PRD §12).
/// </summary>
public sealed record CleanupCandidateInfo(
    CleanupRuleId RuleId,
    string Name,
    string Description,
    string PathDisplay,
    string Reason,
    string Consequence,
    RiskLevel RiskLevel,
    ulong TotalBytes,
    ulong FileCount,
    bool IsProtected
);

/// <summary>
/// Summary report returned upon executing or simulating cleanup (ADR-013).
/// </summary>
public sealed record CleanupExecutionReport(
    ulong FilesReclaimed,
    ulong BytesReclaimed,
    ulong FilesFailed,
    bool IsDryRun
);

/// <summary>
/// Managed service providing safe cleanup candidate detection and execution (ADR-013).
/// </summary>
public static unsafe class CleanupService
{
    /// <summary>
    /// Detects all cleanup candidates across the active StorageTree and system locations.
    /// </summary>
    public static CleanupCandidateInfo[] DetectCandidates(StorageTreeService tree)
    {
        if (tree == null) throw new ArgumentNullException(nameof(tree));

        uint initialCap = 16;
        var buffer = new CleanupCandidateFfi[initialCap];
        uint count = 0;
        uint total = 0;

        fixed (CleanupCandidateFfi* ptr = buffer)
        {
            int code = NativeMethods.cleanup_detect_candidates(
                tree.Handle.DangerousHandle,
                ptr,
                initialCap,
                &count,
                &total);

            if (code != 0)
            {
                throw new ScanException(code, "Failed to detect cleanup candidates");
            }
        }

        var results = new CleanupCandidateInfo[count];
        for (int i = 0; i < count; i++)
        {
            var ffi = buffer[i];
            var rule = (CleanupRuleId)ffi.rule_id;
            var (name, desc, path, reason, conseq) = GetRuleExplanations(rule);

            results[i] = new CleanupCandidateInfo(
                RuleId: rule,
                Name: name,
                Description: desc,
                PathDisplay: path,
                Reason: reason,
                Consequence: conseq,
                RiskLevel: (RiskLevel)ffi.risk_level,
                TotalBytes: ffi.total_bytes,
                FileCount: ffi.file_count,
                IsProtected: ffi.is_protected != 0
            );
        }

        return results;
    }

    /// <summary>
    /// Detects cleanup candidates asynchronously on a background thread.
    /// </summary>
    public static Task<CleanupCandidateInfo[]> DetectCandidatesAsync(StorageTreeService tree)
    {
        return Task.Run(() => DetectCandidates(tree));
    }

    /// <summary>
    /// Executes or simulates removal for a specific rule category (ADR-013).
    /// </summary>
    public static CleanupExecutionReport ExecuteRule(
        StorageTreeService tree,
        CleanupRuleId ruleId,
        bool dryRun,
        bool sendToRecycleBin = true)
    {
        if (tree == null) throw new ArgumentNullException(nameof(tree));

        CleanupReportFfi report = default;
        int code = NativeMethods.cleanup_execute_rule(
            tree.Handle.DangerousHandle,
            (uint)ruleId,
            (byte)(dryRun ? 1 : 0),
            (byte)(sendToRecycleBin ? 1 : 0),
            &report);

        if (code != 0)
        {
            throw new ScanException(code, $"Failed to execute cleanup rule {ruleId}");
        }

        return new CleanupExecutionReport(
            FilesReclaimed: report.files_reclaimed,
            BytesReclaimed: report.bytes_reclaimed,
            FilesFailed: report.files_failed,
            IsDryRun: report.is_dry_run != 0
        );
    }

    /// <summary>
    /// Executes cleanup rule asynchronously on a background thread.
    /// </summary>
    public static Task<CleanupExecutionReport> ExecuteRuleAsync(
        StorageTreeService tree,
        CleanupRuleId ruleId,
        bool dryRun,
        bool sendToRecycleBin = true)
    {
        return Task.Run(() => ExecuteRule(tree, ruleId, dryRun, sendToRecycleBin));
    }

    /// <summary>
    /// Checks whether a given path is an immutable protected system location (Invariant 1).
    /// </summary>
    public static bool IsPathProtected(string path)
    {
        if (string.IsNullOrWhiteSpace(path)) return true;
        fixed (char* ptr = path)
        {
            return NativeMethods.cleanup_is_path_protected((ushort*)ptr) != 0;
        }
    }

    private static (string Name, string Description, string Path, string Reason, string Consequence) GetRuleExplanations(CleanupRuleId rule) => rule switch
    {
        CleanupRuleId.UserTemp => (
            "User Temporary Files",
            "Temporary data created by applications and installers.",
            "%LOCALAPPDATA%\\Temp",
            "Applications create working files here and frequently fail to delete them.",
            "Running applications recreate temporary files as needed."
        ),
        CleanupRuleId.SystemTemp => (
            "System Temporary Files",
            "Temporary files generated by Windows system services.",
            "C:\\Windows\\Temp",
            "Leftovers from Windows servicing and background installations.",
            "Safe to remove; services regenerate fresh temporary files."
        ),
        CleanupRuleId.WindowsUpdate => (
            "Windows Update Download Cache",
            "Staged update payloads for already installed updates.",
            "C:\\Windows\\SoftwareDistribution\\Download",
            "Retained after updates have successfully applied.",
            "Windows Update will re-download files if a rollback or repair is needed."
        ),
        CleanupRuleId.CrashDumps => (
            "System Crash Dumps & Minidumps",
            "Memory dump files from past crashes and blue screens.",
            "C:\\Windows\\Minidump & MEMORY.DMP",
            "Post-mortem crash traces no longer under active debugging.",
            "Historical crash logs are purged; future crashes will generate new dumps."
        ),
        CleanupRuleId.Thumbcache => (
            "Windows Thumbnail Cache",
            "Cached image and video thumbnail preview databases.",
            "%LOCALAPPDATA%\\Microsoft\\Windows\\Explorer",
            "Thumbnails accumulate for files that may have been moved or deleted.",
            "File Explorer rebuilds thumbnails on demand when folders are opened."
        ),
        CleanupRuleId.RecycleBin => (
            "Recycle Bin Contents",
            "Files previously deleted by user waiting to be emptied.",
            "$Recycle.Bin",
            "Holds deleted files until permanently cleared.",
            "Permanently frees storage; items can no longer be restored from the Recycle Bin."
        ),
        CleanupRuleId.StaleLogs => (
            "Stale Diagnostic Logs",
            "Old log files (*.log, *.bak, *.old) in temporary folders.",
            "*.log, *.bak in temp/cache",
            "Old diagnostic traces from completed operations.",
            "Historical log entries removed without affecting application operation."
        ),
        _ => ("Unknown Category", "System cache files", "—", "Stale cache", "No effect")
    };
}
