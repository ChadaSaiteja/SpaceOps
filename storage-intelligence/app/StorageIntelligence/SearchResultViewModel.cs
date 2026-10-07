using System;
using Microsoft.UI;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using Windows.UI;

namespace StorageIntelligence;

/// <summary>
/// Presentation model for a search hit displayed in the AutoSuggestBox flyout (ADR-012).
/// </summary>
public sealed class SearchResultViewModel
{
    public ulong NodeId { get; }
    public string Name { get; }
    public string FullPath { get; }
    public ulong Size { get; }
    public NodeKind Kind { get; }
    public FileCategory Category { get; }
    public uint Score { get; }

    public string FormattedSize => FormatBytes(Size);
    public string CategoryText => Kind == NodeKind.Directory ? "Folder" : $"{Category}";
    public string Glyph => Kind == NodeKind.Directory ? "\uE8B7" : GetCategoryGlyph(Category);
    public SolidColorBrush CategoryBrush { get; }

    public SearchResultViewModel(SearchResultItem item)
    {
        NodeId = item.NodeId;
        Name = item.Name;
        FullPath = item.FullPath;
        Size = item.Size;
        Kind = item.Kind;
        Category = item.Category;
        Score = item.Score;

        Color col = Kind == NodeKind.Directory ? Color.FromArgb(255, 234, 179, 8) : GetCategoryColor(Category);
        CategoryBrush = new SolidColorBrush(col);
    }

    private static string GetCategoryGlyph(FileCategory cat) => cat switch
    {
        FileCategory.Video => "\uE714",       // Media
        FileCategory.Audio => "\uE8D6",       // Audio
        FileCategory.Image => "\uEB9F",       // Photo
        FileCategory.Document => "\uE8A5",    // Document
        FileCategory.Archive => "\uF012",     // Zip / Archive
        FileCategory.Executable => "\uE756",  // Executable / Application
        FileCategory.Code => "\uE943",        // Code
        FileCategory.System => "\uE770",      // System
        _ => "\uE7C3",                        // File
    };

    private static Color GetCategoryColor(FileCategory cat) => cat switch
    {
        FileCategory.Video => Color.FromArgb(255, 138, 43, 226),
        FileCategory.Audio => Color.FromArgb(255, 255, 20, 147),
        FileCategory.Image => Color.FromArgb(255, 255, 140, 0),
        FileCategory.Document => Color.FromArgb(255, 30, 144, 255),
        FileCategory.Archive => Color.FromArgb(255, 50, 205, 50),
        FileCategory.Executable => Color.FromArgb(255, 220, 20, 60),
        FileCategory.Code => Color.FromArgb(255, 0, 206, 209),
        FileCategory.System => Color.FromArgb(255, 112, 128, 144),
        _ => Color.FromArgb(255, 105, 105, 105),
    };

    private static string FormatBytes(ulong bytes)
    {
        string[] units = { "B", "KB", "MB", "GB", "TB" };
        double len = bytes;
        int order = 0;
        while (len >= 1024.0 && order < units.Length - 1)
        {
            order++;
            len /= 1024.0;
        }
        return $"{len:0.##} {units[order]}";
    }
}
