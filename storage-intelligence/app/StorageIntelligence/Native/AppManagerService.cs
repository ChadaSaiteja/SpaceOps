using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>
/// Packaging / runtime deployment format for installed software (ADR-014).
/// </summary>
public enum AppKind : uint
{
    Win32 = 0,
    Msix = 1,
}

/// <summary>
/// Location category for application cache and data residue.
/// </summary>
public enum LeftoverLocationType : uint
{
    LocalAppData = 0,
    AppDataRoaming = 1,
    ProgramData = 2,
}

/// <summary>
/// Managed record representing an installed software package with high-fidelity storage attribution (ADR-014).
/// </summary>
public sealed record InstalledAppInfo(
    string Id,
    string Name,
    string Version,
    string Publisher,
    string InstallLocation,
    string UninstallString,
    string QuietUninstallString,
    string InstallDate,
    AppKind Kind,
    ulong EstimatedSizeBytes,
    ulong ActualSizeBytes,
    ulong FileCount,
    bool IsSystemComponent
)
{
    public ulong DisplaySizeBytes => ActualSizeBytes > 0 ? ActualSizeBytes : EstimatedSizeBytes;
    public string FormattedSize => DisplaySizeBytes > 0 ? FormatBytes(DisplaySizeBytes) : "Unknown";
    public string KindText => Kind == AppKind.Win32 ? "Desktop App" : "Store / MSIX";

    public static string FormatBytes(ulong bytes)
    {
        if (bytes >= 1024UL * 1024 * 1024 * 1024)
            return $"{bytes / (1024.0 * 1024 * 1024 * 1024):0.##} TB";
        if (bytes >= 1024UL * 1024 * 1024)
            return $"{bytes / (1024.0 * 1024 * 1024):0.##} GB";
        if (bytes >= 1024UL * 1024)
            return $"{bytes / (1024.0 * 1024):0.##} MB";
        if (bytes >= 1024UL)
            return $"{bytes / 1024.0:0.##} KB";
        return $"{bytes} B";
    }
}

/// <summary>
/// Managed record representing an identified orphaned application residue folder (ADR-014 §4).
/// </summary>
public sealed record AppLeftoverInfo(
    string Id,
    string AppName,
    string Path,
    ulong SizeBytes,
    ulong FileCount,
    LeftoverLocationType LocationType,
    RiskLevel RiskLevel
)
{
    public string FormattedSize => InstalledAppInfo.FormatBytes(SizeBytes);
    public string LocationTypeText => LocationType switch
    {
        LeftoverLocationType.LocalAppData => "Local AppData",
        LeftoverLocationType.AppDataRoaming => "Roaming AppData",
        LeftoverLocationType.ProgramData => "ProgramData",
        _ => "Application Cache",
    };
}

/// <summary>
/// Managed coordinator for Application Management and Storage Attribution (ADR-014, PRD §14).
/// </summary>
public static class AppManagerService
{
    /// <summary>
    /// Discovers installed software across Win32 registry roots and MSIX packages,
    /// attributing exact cluster sizes from the active StorageTree where available.
    /// </summary>
    public static unsafe List<InstalledAppInfo> DiscoverApplications(StorageTreeService? tree = null)
    {
        var apps = new List<InstalledAppInfo>();

        // 1. Discover Win32 applications via native Rust registry engine
        AppCatalogHandle* catalog = null;
        try
        {
            TreeHandle* nativeTree = tree != null ? tree.Handle.DangerousHandle : null;
            int code = NativeMethods.app_catalog_create(nativeTree, &catalog);
            if (code == 0 && catalog != null)
            {
                uint count = NativeMethods.app_catalog_count(catalog);
                Span<char> charBuf = stackalloc char[512];

                for (uint i = 0; i < count; i++)
                {
                    AppInfoFfi ffi = default;
                    if (NativeMethods.app_catalog_item(catalog, i, &ffi) != 0) continue;

                    string id = ReadStringField(catalog, i, 0, charBuf);
                    string name = ReadStringField(catalog, i, 1, charBuf);
                    string version = ReadStringField(catalog, i, 2, charBuf);
                    string publisher = ReadStringField(catalog, i, 3, charBuf);
                    string installLoc = ReadStringField(catalog, i, 4, charBuf);
                    string uninst = ReadStringField(catalog, i, 5, charBuf);
                    string quietUninst = ReadStringField(catalog, i, 6, charBuf);
                    string installDate = ReadStringField(catalog, i, 7, charBuf);

                    apps.Add(new InstalledAppInfo(
                        Id: id,
                        Name: string.IsNullOrWhiteSpace(name) ? id : name,
                        Version: version,
                        Publisher: publisher,
                        InstallLocation: installLoc,
                        UninstallString: uninst,
                        QuietUninstallString: quietUninst,
                        InstallDate: installDate,
                        Kind: (AppKind)ffi.kind,
                        EstimatedSizeBytes: ffi.estimated_size,
                        ActualSizeBytes: ffi.actual_size,
                        FileCount: ffi.file_count,
                        IsSystemComponent: ffi.is_system_component != 0
                    ));
                }
            }
        }
        finally
        {
            if (catalog != null)
            {
                NativeMethods.app_catalog_destroy(catalog);
            }
        }

        // 2. Discover MSIX / AppX packages via WinRT PackageManager
        try
        {
            var packageManager = new Windows.Management.Deployment.PackageManager();
            var packages = packageManager.FindPackagesForUser("");
            foreach (var pkg in packages)
            {
                if (pkg.IsFramework || pkg.IsResourcePackage) continue;

                string displayName = "";
                try { displayName = pkg.DisplayName; } catch { }
                if (string.IsNullOrWhiteSpace(displayName))
                {
                    try { displayName = pkg.Id.Name; } catch { }
                }
                if (string.IsNullOrWhiteSpace(displayName)) continue;

                string installPath = "";
                try { installPath = pkg.InstalledLocation?.Path ?? ""; } catch { }

                string publisher = "";
                try { publisher = pkg.PublisherDisplayName; } catch { }

                ulong actualSize = 0;
                ulong fileCount = 0;

                // Attribute size from tree if available
                if (!string.IsNullOrEmpty(installPath) && tree != null)
                {
                    var node = tree.FindByPath(installPath);
                    if (node != null)
                    {
                        actualSize = node.Value.Size;
                        fileCount = node.Value.FileCount;
                    }
                }

                apps.Add(new InstalledAppInfo(
                    Id: pkg.Id.FullName,
                    Name: displayName,
                    Version: $"{pkg.Id.Version.Major}.{pkg.Id.Version.Minor}.{pkg.Id.Version.Build}",
                    Publisher: publisher,
                    InstallLocation: installPath,
                    UninstallString: "",
                    QuietUninstallString: "",
                    InstallDate: "",
                    Kind: AppKind.Msix,
                    EstimatedSizeBytes: 0,
                    ActualSizeBytes: actualSize,
                    FileCount: fileCount,
                    IsSystemComponent: false
                ));
            }
        }
        catch
        {
            // Ignored in execution environments where WinRT PackageManager is unavailable
        }

        // Sort descending by true disk size, then estimated size, then name
        apps.Sort((a, b) =>
        {
            int cmp = b.DisplaySizeBytes.CompareTo(a.DisplaySizeBytes);
            if (cmp != 0) return cmp;
            return string.Compare(a.Name, b.Name, StringComparison.OrdinalIgnoreCase);
        });

        return apps;
    }

    /// <summary>
    /// Asynchronously discovers installed applications.
    /// </summary>
    public static Task<List<InstalledAppInfo>> DiscoverApplicationsAsync(StorageTreeService? tree = null)
    {
        return Task.Run(() => DiscoverApplications(tree));
    }

    /// <summary>
    /// Detects orphaned application leftovers across user and system caches (ADR-014 §4).
    /// </summary>
    public static unsafe List<AppLeftoverInfo> DiscoverLeftovers(
        IReadOnlyList<InstalledAppInfo>? installedApps = null,
        StorageTreeService? tree = null)
    {
        var leftovers = new List<AppLeftoverInfo>();
        AppCatalogHandle* catalog = null;
        AppLeftoversHandle* leftoversHandle = null;

        try
        {
            TreeHandle* nativeTree = tree != null ? tree.Handle.DangerousHandle : null;
            NativeMethods.app_catalog_create(nativeTree, &catalog);

            int code = NativeMethods.app_leftovers_detect(catalog, nativeTree, &leftoversHandle);
            if (code == 0 && leftoversHandle != null)
            {
                uint count = NativeMethods.app_leftovers_count(leftoversHandle);
                Span<char> charBuf = stackalloc char[512];

                for (uint i = 0; i < count; i++)
                {
                    AppLeftoverFfi ffi = default;
                    if (NativeMethods.app_leftovers_item(leftoversHandle, i, &ffi) != 0) continue;

                    string id = ReadLeftoverStringField(leftoversHandle, i, 0, charBuf);
                    string appName = ReadLeftoverStringField(leftoversHandle, i, 1, charBuf);
                    string path = ReadLeftoverStringField(leftoversHandle, i, 2, charBuf);

                    leftovers.Add(new AppLeftoverInfo(
                        Id: id,
                        AppName: appName,
                        Path: path,
                        SizeBytes: ffi.size,
                        FileCount: ffi.file_count,
                        LocationType: (LeftoverLocationType)ffi.location_type,
                        RiskLevel: (RiskLevel)ffi.risk_level
                    ));
                }
            }
        }
        finally
        {
            if (leftoversHandle != null)
            {
                NativeMethods.app_leftovers_destroy(leftoversHandle);
            }
            if (catalog != null)
            {
                NativeMethods.app_catalog_destroy(catalog);
            }
        }

        leftovers.Sort((a, b) => b.SizeBytes.CompareTo(a.SizeBytes));
        return leftovers;
    }

    /// <summary>
    /// Asynchronously discovers application leftovers.
    /// </summary>
    public static Task<List<AppLeftoverInfo>> DiscoverLeftoversAsync(
        IReadOnlyList<InstalledAppInfo>? installedApps = null,
        StorageTreeService? tree = null)
    {
        return Task.Run(() => DiscoverLeftovers(installedApps, tree));
    }

    /// <summary>
    /// Prepares or executes uninstallation of an application through vendor-sanctioned channels (ADR-014 §3).
    /// </summary>
    public static async Task<bool> UninstallApplicationAsync(InstalledAppInfo app)
    {
        if (app.IsSystemComponent)
        {
            throw new InvalidOperationException("System components cannot be uninstalled.");
        }

        if (app.Kind == AppKind.Msix)
        {
            var packageManager = new Windows.Management.Deployment.PackageManager();
            var deploymentOp = packageManager.RemovePackageAsync(app.Id);
            var result = await deploymentOp.AsTask();
            return result.IsRegistered == false;
        }

        if (app.Kind == AppKind.Win32)
        {
            string cmd = !string.IsNullOrWhiteSpace(app.QuietUninstallString)
                ? app.QuietUninstallString
                : app.UninstallString;

            if (string.IsNullOrWhiteSpace(cmd))
            {
                throw new InvalidOperationException("No registered uninstaller found for this application.");
            }

            return LaunchUninstallProcess(cmd);
        }

        return false;
    }

    private static bool LaunchUninstallProcess(string commandLine)
    {
        var trimmed = commandLine.Trim();
        string fileName;
        string arguments = "";

        if (trimmed.StartsWith('"'))
        {
            int closingQuote = trimmed.IndexOf('"', 1);
            if (closingQuote > 1)
            {
                fileName = trimmed.Substring(1, closingQuote - 1);
                if (closingQuote + 1 < trimmed.Length)
                {
                    arguments = trimmed.Substring(closingQuote + 1).Trim();
                }
            }
            else
            {
                fileName = trimmed.Trim('"');
            }
        }
        else
        {
            int spaceIdx = trimmed.IndexOf(' ');
            if (spaceIdx > 0)
            {
                fileName = trimmed.Substring(0, spaceIdx);
                arguments = trimmed.Substring(spaceIdx + 1).Trim();
            }
            else
            {
                fileName = trimmed;
            }
        }

        var psi = new ProcessStartInfo
        {
            FileName = fileName,
            Arguments = arguments,
            UseShellExecute = true,
            Verb = "runas" // Elevate if required
        };

        var proc = Process.Start(psi);
        return proc != null;
    }

    private static unsafe string ReadStringField(AppCatalogHandle* catalog, uint index, uint fieldId, Span<char> buffer)
    {
        uint actualLen = 0;
        fixed (char* bufPtr = buffer)
        {
            int code = NativeMethods.app_catalog_string(
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
                int code = NativeMethods.app_catalog_string(
                    catalog, index, fieldId, (ushort*)heapPtr, (uint)heapBuf.Length, &actualLen);
                if (code == 0)
                {
                    return new string(heapBuf, 0, (int)actualLen);
                }
            }
        }

        return string.Empty;
    }

    private static unsafe string ReadLeftoverStringField(AppLeftoversHandle* leftovers, uint index, uint fieldId, Span<char> buffer)
    {
        uint actualLen = 0;
        fixed (char* bufPtr = buffer)
        {
            int code = NativeMethods.app_leftovers_string(
                leftovers, index, fieldId, (ushort*)bufPtr, (uint)buffer.Length, &actualLen);

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
                int code = NativeMethods.app_leftovers_string(
                    leftovers, index, fieldId, (ushort*)heapPtr, (uint)heapBuf.Length, &actualLen);
                if (code == 0)
                {
                    return new string(heapBuf, 0, (int)actualLen);
                }
            }
        }

        return string.Empty;
    }
}
