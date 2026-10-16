using System;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using StorageIntelligence.Styles;
using Windows.UI;

namespace StorageIntelligence;

/// <summary>
/// Presentation model for a cleanup candidate card with two-way selection and explanations (PRD §12).
/// </summary>
public sealed class CleanupCandidateViewModel : INotifyPropertyChanged
{
    private bool _isSelected;

    public CleanupRuleId RuleId { get; }
    public string Name { get; }
    public string Description { get; }
    public string PathDisplay { get; }
    public string Reason { get; }
    public string Consequence { get; }
    public RiskLevel RiskLevel { get; }
    public ulong TotalBytes { get; }
    public ulong FileCount { get; }
    public bool IsProtected { get; }

    public bool IsSelected
    {
        get => _isSelected;
        set
        {
            if (_isSelected != value)
            {
                _isSelected = value;
                OnPropertyChanged();
            }
        }
    }

    public string FormattedSize => FormatBytes(TotalBytes);
    public string FileCountText => $"{FileCount:N0} files";
    public string RiskText => RiskLevel switch
    {
        RiskLevel.Low => "Low Risk",
        RiskLevel.Medium => "Medium Risk",
        RiskLevel.High => "Review Needed",
        _ => "Standard"
    };

    public SolidColorBrush RiskBrush => RiskLevel switch
    {
        RiskLevel.Low => ThemeBrushes.Green,
        RiskLevel.Medium => ThemeBrushes.Amber,
        RiskLevel.High => ThemeBrushes.Red,
        _ => ThemeBrushes.Neutral
    };

    public CleanupCandidateViewModel(CleanupCandidateInfo info)
    {
        RuleId = info.RuleId;
        Name = info.Name;
        Description = info.Description;
        PathDisplay = info.PathDisplay;
        Reason = info.Reason;
        Consequence = info.Consequence;
        RiskLevel = info.RiskLevel;
        TotalBytes = info.TotalBytes;
        FileCount = info.FileCount;
        IsProtected = info.IsProtected;

        // Low risk items are selected by default; medium/high require conscious opt-in
        _isSelected = info.RiskLevel == RiskLevel.Low && info.TotalBytes > 0 && !info.IsProtected;
    }

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

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? propertyName = null)
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
    }
}
