using System;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using Windows.UI;

namespace StorageIntelligence;

/// <summary>
/// Presentation model for an installed application card with true storage attribution (ADR-014).
/// </summary>
public sealed class InstalledAppViewModel : INotifyPropertyChanged
{
    public InstalledAppInfo Info { get; }

    public string Id => Info.Id;
    public string Name => Info.Name;
    public string Version => string.IsNullOrEmpty(Info.Version) ? "—" : Info.Version;
    public string Publisher => string.IsNullOrEmpty(Info.Publisher) ? "Unknown Publisher" : Info.Publisher;
    public string InstallLocation => string.IsNullOrEmpty(Info.InstallLocation) ? "—" : Info.InstallLocation;
    public AppKind Kind => Info.Kind;
    public ulong DisplaySizeBytes => Info.DisplaySizeBytes;
    public ulong FileCount => Info.FileCount;
    public bool IsSystemComponent => Info.IsSystemComponent;

    public string FormattedSize => Info.FormattedSize;
    public string FileCountText => FileCount > 0 ? $"{FileCount:N0} files" : (Info.ActualSizeBytes > 0 ? "Scanned" : "Registry Est.");
    public string KindText => Info.KindText;

    public string IconGlyph => Kind == AppKind.Msix ? "\uE74C" : "\uE71D";

    public bool HasInstallLocation => !string.IsNullOrWhiteSpace(Info.InstallLocation) && Info.InstallLocation != "—";
    public bool CanUninstall => !IsSystemComponent && (!string.IsNullOrWhiteSpace(Info.UninstallString) || Kind == AppKind.Msix);

    public SolidColorBrush KindBrush => Kind == AppKind.Msix
        ? new SolidColorBrush(Color.FromArgb(255, 59, 130, 246))  // Blue for Store/MSIX
        : new SolidColorBrush(Color.FromArgb(255, 139, 92, 246)); // Purple for Win32 Desktop

    public InstalledAppViewModel(InstalledAppInfo info)
    {
        Info = info ?? throw new ArgumentNullException(nameof(info));
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
