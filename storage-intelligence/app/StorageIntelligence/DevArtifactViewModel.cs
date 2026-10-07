using System;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
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
        DevEcosystem.NodeJs => new SolidColorBrush(Color.FromArgb(255, 16, 185, 129)),  // Green
        DevEcosystem.Rust => new SolidColorBrush(Color.FromArgb(255, 249, 115, 22)),   // Orange
        DevEcosystem.DotNet => new SolidColorBrush(Color.FromArgb(255, 139, 92, 246)), // Purple
        DevEcosystem.Python => new SolidColorBrush(Color.FromArgb(255, 59, 130, 246)),  // Blue
        DevEcosystem.Java => new SolidColorBrush(Color.FromArgb(255, 217, 119, 6)),    // Amber
        DevEcosystem.DockerWsl => new SolidColorBrush(Color.FromArgb(255, 6, 182, 212)),// Cyan
        DevEcosystem.Git => new SolidColorBrush(Color.FromArgb(255, 239, 68, 68)),     // Red
        _ => new SolidColorBrush(Color.FromArgb(255, 156, 163, 175)),                  // Gray
    };

    public SolidColorBrush DormancyBrush => IsDormant
        ? new SolidColorBrush(Color.FromArgb(255, 245, 158, 11))   // Amber
        : new SolidColorBrush(Color.FromArgb(255, 16, 185, 129));  // Green

    public SolidColorBrush RiskBrush => RiskLevel switch
    {
        RiskLevel.Low => new SolidColorBrush(Color.FromArgb(255, 16, 185, 129)),
        RiskLevel.Medium => new SolidColorBrush(Color.FromArgb(255, 245, 158, 11)),
        _ => new SolidColorBrush(Color.FromArgb(255, 239, 68, 68)),
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
