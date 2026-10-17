using System;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using StorageIntelligence.Styles;
using Windows.UI;

namespace StorageIntelligence;

/// <summary>
/// Presentation model for a developer storage artifact card with dormancy and reclamation controls (ADR-015).
/// </summary>
public sealed class DevArtifactViewModel : INotifyPropertyChanged
{
    public DevArtifactInfo Info { get; }

    public string Id => Info.Id;
    public string Name => Info.Name;
    public string ProjectName => Info.ProjectName;
    public string Path => Info.Path;
    public ulong SizeBytes => Info.SizeBytes;
    public ulong FileCount => Info.FileCount;
    public bool IsDormant => Info.IsDormant;
    public uint DaysInactive => Info.DaysInactive;
    public RiskLevel RiskLevel => Info.RiskLevel;
    public string Description => Info.Description;
    public string CleanupCommand => Info.CleanupCommand;

    public string FormattedSize => Info.FormattedSize;
    public string FileCountText => FileCount > 0 ? $"{FileCount:N0} files" : "Single entity";
    public string EcosystemName => Info.EcosystemName;
    public string KindName => Info.KindName;
    public string DormancyText => Info.DormancyText;

    public bool HasCleanupCommand => !string.IsNullOrWhiteSpace(CleanupCommand);
    public bool CanDirectClean => RiskLevel != RiskLevel.High;

    public string IconGlyph => Info.Ecosystem switch
    {
        DevEcosystem.NodeJs => "\uE74C",    // App/Script
        DevEcosystem.Rust => "\uE7C3",      // Gear/Engine
        DevEcosystem.DotNet => "\uE790",    // Code
        DevEcosystem.Python => "\uE8B7",    // Command
        DevEcosystem.Java => "\uE943",      // Coffee/Box
        DevEcosystem.DockerWsl => "\uE838",  // Server/Container
        DevEcosystem.Git => "\uE81E",       // Branch
        _ => "\uE7B8",                      // Tool
    };

    public SolidColorBrush EcosystemBrush => Info.Ecosystem switch
    {
        DevEcosystem.NodeJs => ThemeBrushes.Green,
        DevEcosystem.Rust => ThemeBrushes.Orange,
        DevEcosystem.DotNet => ThemeBrushes.Purple,
        DevEcosystem.Python => ThemeBrushes.Accent2,
        DevEcosystem.Java => ThemeBrushes.Amber,
        DevEcosystem.DockerWsl => ThemeBrushes.Cyan,
        DevEcosystem.Git => ThemeBrushes.Red,
        _ => ThemeBrushes.Neutral,
    };

    public SolidColorBrush DormancyBrush => IsDormant
        ? ThemeBrushes.Amber
        : (Info.IsActivityKnown ? ThemeBrushes.Green : ThemeBrushes.Muted);

    public SolidColorBrush RiskBrush => RiskLevel switch
    {
        RiskLevel.Low => ThemeBrushes.Green,
        RiskLevel.Medium => ThemeBrushes.Amber,
        _ => ThemeBrushes.Red,
    };

    public string RiskText => RiskLevel switch
    {
        RiskLevel.Low => "Low Risk • Recreated on build",
        RiskLevel.Medium => "Medium Risk • Re-downloaded when needed",
        _ => "High Risk • Docker/VM Data",
    };

    public DevArtifactViewModel(DevArtifactInfo info)
    {
        Info = info ?? throw new ArgumentNullException(nameof(info));
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
