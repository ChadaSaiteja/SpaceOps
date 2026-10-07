using System;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>
/// Programming ecosystem / toolchain classification (ADR-015 §1).
/// </summary>
public enum DevEcosystem : uint
{
    NodeJs = 0,
    Rust = 1,
    DotNet = 2,
    Python = 3,
    Java = 4,
    DockerWsl = 5,
    Git = 6,
    Other = 7,
}

/// <summary>
/// Category of developer storage artifact (ADR-015).
/// </summary>
public enum DevArtifactKind : uint
{
    ProjectArtifact = 0, // node_modules, target/, bin/obj, .venv
    GlobalCache = 1,     // NuGet packages, Cargo cache, npm cache, pip cache
    VirtualDisk = 2,     // Docker ext4.vhdx, WSL vhdx
}

/// <summary>
/// Managed record representing a discovered developer artifact with storage, dormancy, and safety attributes (ADR-015).
/// </summary>
public sealed record DevArtifactInfo(
    string Id,
    DevEcosystem Ecosystem,
    DevArtifactKind Kind,
    string Name,
    string ProjectName,
    string Path,
    ulong SizeBytes,
    ulong FileCount,
    bool IsDormant,
    uint DaysInactive,
    RiskLevel RiskLevel,
    string Description,
    string CleanupCommand
)
{
    public string FormattedSize => InstalledAppInfo.FormatBytes(SizeBytes);

    public string EcosystemName => Ecosystem switch
    {
        DevEcosystem.NodeJs => "Node.js / JS",
        DevEcosystem.Rust => "Rust / Cargo",
        DevEcosystem.DotNet => ".NET / C#",
        DevEcosystem.Python => "Python / pip",
        DevEcosystem.Java => "Java / Gradle",
        DevEcosystem.DockerWsl => "Docker & WSL",
        DevEcosystem.Git => "Git Repositories",
        _ => "Other Tools",
    };

    public string KindName => Kind switch
    {
        DevArtifactKind.ProjectArtifact => "Project Artifacts",
        DevArtifactKind.GlobalCache => "Global Package Cache",
        DevArtifactKind.VirtualDisk => "Container / WSL Disk",
        _ => "Developer Storage",
    };

    public string DormancyText => IsDormant
        ? $"Dormant ({DaysInactive} days inactive)"
        : (DaysInactive > 0 ? $"Active ({DaysInactive} days ago)" : "Active today");
}

/// <summary>
/// Managed service providing developer storage discovery, dormancy analysis, and safe reclamation (ADR-015).
/// </summary>
public static unsafe class DevStorageService
{
    /// <summary>
    /// Synchronously discovers developer artifacts across active tree and canonical global paths.
    /// </summary>
    public static List<DevArtifactInfo> DiscoverArtifacts(StorageTreeService? tree = null)
    {
        var artifacts = new List<DevArtifactInfo>();
        DevCatalogHandle* catalog = null;

        try
        {
            TreeHandle* nativeTree = tree != null ? tree.Handle.DangerousHandle : null;
            int code = NativeMethods.dev_catalog_create(nativeTree, &catalog);
            if (code == 0 && catalog != null)
            {
                uint count = NativeMethods.dev_catalog_count(catalog);
                Span<char> charBuf = stackalloc char[512];

                for (uint i = 0; i < count; i++)
                {
                    DevArtifactFfi ffi = default;
                    if (NativeMethods.dev_catalog_item(catalog, i, &ffi) != 0) continue;

                    string id = ReadStringField(catalog, i, 0, charBuf);
                    string name = ReadStringField(catalog, i, 1, charBuf);
                    string projectName = ReadStringField(catalog, i, 2, charBuf);
                    string path = ReadStringField(catalog, i, 3, charBuf);
                    string desc = ReadStringField(catalog, i, 4, charBuf);
                    string cmd = ReadStringField(catalog, i, 5, charBuf);

                    artifacts.Add(new DevArtifactInfo(
                        Id: id,
                        Ecosystem: (DevEcosystem)ffi.ecosystem,
                        Kind: (DevArtifactKind)ffi.kind,
                        Name: name,
                        ProjectName: projectName,
                        Path: path,
                        SizeBytes: ffi.size,
                        FileCount: ffi.file_count,
                        IsDormant: ffi.is_dormant != 0,
                        DaysInactive: ffi.days_inactive,
                        RiskLevel: (RiskLevel)ffi.risk_level,
                        Description: desc,
                        CleanupCommand: cmd
                    ));
                }
            }
        }
        finally
        {
            if (catalog != null)
            {
                NativeMethods.dev_catalog_destroy(catalog);
            }
        }

        // Sort descending by size
        artifacts.Sort((a, b) => b.SizeBytes.CompareTo(a.SizeBytes));
        return artifacts;
    }

    /// <summary>
    /// Asynchronously discovers developer artifacts on a background thread.
    /// </summary>
    public static Task<List<DevArtifactInfo>> DiscoverArtifactsAsync(StorageTreeService? tree = null)
    {
        return Task.Run(() => DiscoverArtifacts(tree));
    }

    /// <summary>
    /// Safely reclaims a developer artifact using Recycle Bin undo support and guardrails (ADR-015 §4).
    /// </summary>
    public static CleanupExecutionReport CleanArtifact(
        string path,
        bool dryRun = false,
        bool sendToRecycleBin = true)
    {
        if (string.IsNullOrWhiteSpace(path))
            throw new ArgumentException("Path cannot be empty", nameof(path));

        CleanupReportFfi report = default;
        fixed (char* pathPtr = path)
        {
            int code = NativeMethods.dev_clean_artifact(
                (ushort*)pathPtr,
                (byte)(dryRun ? 1 : 0),
                (byte)(sendToRecycleBin ? 1 : 0),
                &report);

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to clean developer artifact: {path}");
            }
        }

        return new CleanupExecutionReport(
            FilesReclaimed: report.files_reclaimed,
            BytesReclaimed: report.bytes_reclaimed,
            FilesFailed: report.files_failed,
            IsDryRun: report.is_dry_run != 0
        );
    }

    /// <summary>
    /// Asynchronously reclaims a developer artifact on a background thread.
    /// </summary>
    public static Task<CleanupExecutionReport> CleanArtifactAsync(
        string path,
        bool dryRun = false,
        bool sendToRecycleBin = true)
    {
        return Task.Run(() => CleanArtifact(path, dryRun, sendToRecycleBin));
    }

    private static string ReadStringField(DevCatalogHandle* catalog, uint index, uint fieldId, Span<char> buffer)
    {
        uint actualLen = 0;
        fixed (char* bufPtr = buffer)
        {
            int code = NativeMethods.dev_catalog_string(
                catalog, index, fieldId, (ushort*)bufPtr, (uint)buffer.Length, &actualLen);

            if (code == 0)
            {
                return new string(buffer[..(int)actualLen]);
            }
        }

        if (actualLen > 0)
        {
            var heapBuf = new char[actualLen + 1];
            fixed (char* heapPtr = heapBuf)
            {
                int code = NativeMethods.dev_catalog_string(
                    catalog, index, fieldId, (ushort*)heapPtr, (uint)heapBuf.Length, &actualLen);
                if (code == 0)
                {
                    return new string(heapBuf, 0, (int)actualLen);
                }
            }
        }

        return string.Empty;
    }
}
