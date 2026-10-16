using Microsoft.UI.Xaml.Media;

namespace StorageIntelligence.Pages;

/// <summary>One legend row under the usage donut.</summary>
public sealed record SliceRowViewModel(string Name, Brush Brush, string SizeText, string PercentText);

/// <summary>One of the top-5 consumers.</summary>
public sealed record ConsumerRowViewModel(
    ulong NodeId,
    string Name,
    string Glyph,
    string SizeText,
    string PercentText,
    double Percent,
    Brush Brush);
