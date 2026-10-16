using System;
using System.Collections.Generic;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using Windows.UI;

namespace StorageIntelligence.Styles;

/// <summary>One legend / donut entry for a file category.</summary>
public sealed record CategoryLegendItem(FileCategory Category, string Name, string Hex, SolidColorBrush Brush);

/// <summary>
/// Single source of truth for treemap file-category colors and display names.
/// Everything that needs a category color (Direct2D treemap, legend, search results,
/// overview donut, <see cref="TreemapService.GetCategoryColorHex"/>) reads it from here.
/// UI chrome colors (surfaces, accent, text, semantic) live in Styles/Theme.xaml.
/// The hex accessors have no XAML dependency, so they stay usable from non-UI test code.
/// </summary>
public static class Palette
{
    public static string CategoryHex(FileCategory category) => category switch
    {
        FileCategory.Video => "#9D4EDD",       // Purple
        FileCategory.Audio => "#F72585",       // Pink
        FileCategory.Image => "#FF9F1C",       // Amber
        FileCategory.Document => "#3A86FF",    // Blue
        FileCategory.Archive => "#34D399",     // Green
        FileCategory.Executable => "#FB7185",  // Red
        FileCategory.Code => "#00F5D4",        // Accent teal
        FileCategory.System => "#64748B",      // Slate
        _ => "#475569",                        // Other
    };

    public static string CategoryName(FileCategory category) => category switch
    {
        FileCategory.Video => "Video",
        FileCategory.Audio => "Audio",
        FileCategory.Image => "Images",
        FileCategory.Document => "Documents",
        FileCategory.Archive => "Archives",
        FileCategory.Executable => "Programs",
        FileCategory.Code => "Code & Data",
        FileCategory.System => "System",
        _ => "Other",
    };

    /// <summary>Color used for directories in search results and folder glyphs.</summary>
    public const string FolderHex = "#FBBF24";

    public static Color ParseHex(string hex)
    {
        if (hex.StartsWith('#')) hex = hex[1..];
        byte r = Convert.ToByte(hex[0..2], 16);
        byte g = Convert.ToByte(hex[2..4], 16);
        byte b = Convert.ToByte(hex[4..6], 16);
        return Color.FromArgb(255, r, g, b);
    }

    public static Color CategoryColor(FileCategory category) => ParseHex(CategoryHex(category));

    public static SolidColorBrush CategoryBrush(FileCategory category) => new(CategoryColor(category));

    public static SolidColorBrush FolderBrush() => new(ParseHex(FolderHex));

    /// <summary>All categories in display order, with brushes (UI thread only).</summary>
    public static IReadOnlyList<CategoryLegendItem> Legend()
    {
        var list = new List<CategoryLegendItem>();
        foreach (FileCategory cat in Enum.GetValues<FileCategory>())
        {
            list.Add(new CategoryLegendItem(cat, CategoryName(cat), CategoryHex(cat), CategoryBrush(cat)));
        }
        return list;
    }
}

/// <summary>
/// Typed access to the brushes/colors declared in Styles/Theme.xaml for code-behind and view models.
/// </summary>
public static class ThemeBrushes
{
    private static SolidColorBrush Get(string key)
    {
        try
        {
            if (Application.Current?.Resources.TryGetValue(key, out var value) == true && value is SolidColorBrush b)
            {
                return b;
            }
        }
        catch
        {
            // fall through to neutral fallback
        }
        return new SolidColorBrush(Color.FromArgb(255, 139, 155, 180));
    }

    public static Color GetColor(string key)
    {
        try
        {
            if (Application.Current?.Resources.TryGetValue(key, out var value) == true && value is Color c)
            {
                return c;
            }
        }
        catch
        {
            // fall through
        }
        return Color.FromArgb(255, 139, 155, 180);
    }

    public static SolidColorBrush Accent => Get("SpaceAccentBrush");
    public static SolidColorBrush Accent2 => Get("SpaceAccent2Brush");
    public static SolidColorBrush Purple => Get("SpacePurpleBrush");
    public static SolidColorBrush Pink => Get("SpacePinkBrush");
    public static SolidColorBrush Green => Get("SpaceGreenBrush");
    public static SolidColorBrush Amber => Get("SpaceAmberBrush");
    public static SolidColorBrush Red => Get("SpaceRedBrush");
    public static SolidColorBrush Orange => Get("SpaceOrangeBrush");
    public static SolidColorBrush Cyan => Get("SpaceCyanBrush");
    public static SolidColorBrush Neutral => Get("SpaceSubBrush");
    public static SolidColorBrush Muted => Get("SpaceMutedBrush");
    public static SolidColorBrush Border => Get("SpaceBorderBrush");
}
