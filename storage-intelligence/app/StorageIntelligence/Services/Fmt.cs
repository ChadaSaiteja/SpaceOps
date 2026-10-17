using System;
using System.Diagnostics;
using System.IO;

namespace StorageIntelligence.Services;

/// <summary>Shared formatting and small shell helpers used by pages and view models.</summary>
public static class Fmt
{
    private static readonly string[] Units = { "B", "KB", "MB", "GB", "TB" };

    public static string Bytes(ulong bytes)
    {
        double len = bytes;
        int order = 0;
        while (len >= 1024.0 && order < Units.Length - 1)
        {
            order++;
            len /= 1024.0;
        }
        return $"{len:0.##} {Units[order]}";
    }

    /// <summary>Opens Explorer on a directory, or selects a file in its folder. Returns false if the path does not exist.</summary>
    public static bool RevealInExplorer(string? path)
    {
        if (string.IsNullOrWhiteSpace(path)) return false;

        if (Directory.Exists(path))
        {
            Process.Start("explorer.exe", path);
            return true;
        }
        if (File.Exists(path))
        {
            Process.Start("explorer.exe", $"/select,\"{path}\"");
            return true;
        }
        return false;
    }
}
