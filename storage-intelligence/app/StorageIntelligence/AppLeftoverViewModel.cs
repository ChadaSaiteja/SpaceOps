using System;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using Windows.UI;

namespace StorageIntelligence;

/// <summary>
/// Presentation model for an identified orphaned application leftover folder (ADR-014 §4).
/// </summary>
public sealed class AppLeftoverViewModel : INotifyPropertyChanged
{
    private bool _isCleaned;

    public AppLeftoverInfo Info { get; }

    public string Id => Info.Id;
    public string AppName => Info.AppName;
    public string Path => Info.Path;
    public ulong SizeBytes => Info.SizeBytes;
    public ulong FileCount => Info.FileCount;
    public LeftoverLocationType LocationType => Info.LocationType;
    public RiskLevel RiskLevel => Info.RiskLevel;

    public string FormattedSize => Info.FormattedSize;
    public string FileCountText => $"{FileCount:N0} files";
    public string LocationTypeText => Info.LocationTypeText;

    public bool IsCleaned
    {
        get => _isCleaned;
        set
        {
            if (_isCleaned != value)
            {
                _isCleaned = value;
                OnPropertyChanged();
                OnPropertyChanged(nameof(StatusText));
                OnPropertyChanged(nameof(CanClean));
            }
        }
    }

    public bool CanClean => !IsCleaned;
    public string StatusText => IsCleaned ? "Moved to Recycle Bin" : "Orphaned Residue";

    public SolidColorBrush RiskBrush => RiskLevel switch
    {
        RiskLevel.Low => new SolidColorBrush(Color.FromArgb(255, 34, 197, 94)),     // Green
        RiskLevel.Medium => new SolidColorBrush(Color.FromArgb(255, 245, 158, 11)), // Orange
        _ => new SolidColorBrush(Color.FromArgb(255, 239, 68, 68))                  // Red
    };

    public AppLeftoverViewModel(AppLeftoverInfo info)
    {
        Info = info ?? throw new ArgumentNullException(nameof(info));
    }

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null) =>
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
