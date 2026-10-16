using System;
using System.Collections.Generic;
using System.IO;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Animation;
using StorageIntelligence.Pages;
using StorageIntelligence.Services;

namespace StorageIntelligence;

/// <summary>
/// Application shell: sidebar navigation, slim header (target path, Scan, Sync index), drive card and status bar.
/// All feature logic lives in the per-destination pages under Pages/ and in <see cref="AppSession"/>.
/// </summary>
public sealed partial class MainPage : Page
{
    private readonly AppSession _session = AppSession.Current;
    private bool _suppressPathEvents;

    private static readonly Dictionary<string, Type> PageMap = new()
    {
        ["overview"] = typeof(OverviewPage),
        ["treemap"] = typeof(TreemapPage),
        ["cleanup"] = typeof(CleanupPage),
        ["developer"] = typeof(DeveloperPage),
        ["applications"] = typeof(ApplicationsPage),
    };

    public MainPage()
    {
        InitializeComponent();

        // Default path to user profile or Windows drive
        string defaultPath = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (string.IsNullOrEmpty(defaultPath) || !Directory.Exists(defaultPath))
        {
            defaultPath = "C:\\";
        }
        _session.TargetPath = defaultPath;
        _suppressPathEvents = true;
        PathTextBox.Text = defaultPath;
        _suppressPathEvents = false;

        _session.StatusChanged += text => StatusTextBlock.Text = text;
        _session.MetricsChanged += text => MetricsTextBlock.Text = text;
        _session.ScanStateChanged += UpdateScanState;
        _session.TreeChanged += RefreshDriveCard;
        _session.TreeUpdated += RefreshDriveCard;
        _session.NavigateRequested += SelectDestination;

        StatusTextBlock.Text = _session.StatusText;
        RefreshDriveCard();
    }

    private void Nav_Loaded(object sender, RoutedEventArgs e)
    {
        if (Nav.SelectedItem == null)
        {
            Nav.SelectedItem = NavOverview;
        }
    }

    private void Nav_SelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (args.SelectedItem is NavigationViewItem item &&
            item.Tag is string tag &&
            PageMap.TryGetValue(tag, out var pageType) &&
            ContentFrame.CurrentSourcePageType != pageType)
        {
            ContentFrame.Navigate(pageType, null, new SuppressNavigationTransitionInfo());
        }
    }

    private void SelectDestination(string destination)
    {
        NavigationViewItem? target = destination switch
        {
            "overview" => NavOverview,
            "treemap" => NavTreemap,
            "cleanup" => NavCleanup,
            "developer" => NavDeveloper,
            "applications" => NavApplications,
            _ => null,
        };
        if (target != null)
        {
            Nav.SelectedItem = target;
        }
    }

    // ----- Header: target path, Scan, Cancel, Sync index -----

    private void PathTextBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        if (_suppressPathEvents) return;
        _session.TargetPath = PathTextBox.Text;
    }

    private void PathTextBox_LostFocus(object sender, RoutedEventArgs e)
    {
        RefreshDriveCard();
    }

    private async void ScanButton_Click(object sender, RoutedEventArgs e)
    {
        _session.TargetPath = PathTextBox.Text;
        await _session.ScanAsync();
    }

    private void CancelButton_Click(object sender, RoutedEventArgs e)
    {
        CancelButton.IsEnabled = false;
        _session.CancelScan();
    }

    private async void SyncButton_Click(object sender, RoutedEventArgs e)
    {
        _session.TargetPath = PathTextBox.Text;
        await _session.SyncAsync();
    }

    private void UpdateScanState()
    {
        ScanButton.IsEnabled = !_session.IsScanning;
        CancelButton.IsEnabled = _session.IsScanning;
        SyncButton.IsEnabled = !_session.IsSyncing;
        ScanProgressBar.Visibility = _session.IsScanning ? Visibility.Visible : Visibility.Collapsed;
    }

    // ----- Sidebar drive card -----

    private void RefreshDriveCard()
    {
        try
        {
            string? root = Path.GetPathRoot(_session.TargetPath.Trim());
            if (string.IsNullOrEmpty(root)) throw new InvalidOperationException();

            var drive = new DriveInfo(root);
            if (!drive.IsReady) throw new InvalidOperationException();

            ulong total = (ulong)drive.TotalSize;
            ulong free = (ulong)drive.AvailableFreeSpace;
            ulong used = total > free ? total - free : 0;

            DriveNameText.Text = $"{root.TrimEnd('\\')} drive";
            DriveUsageBar.Value = total > 0 ? used * 100.0 / total : 0;
            DriveUsageText.Text = $"{Fmt.Bytes(used)} used of {Fmt.Bytes(total)}  •  {Fmt.Bytes(free)} free";
        }
        catch
        {
            DriveNameText.Text = "Drive";
            DriveUsageBar.Value = 0;
            DriveUsageText.Text = "Usage unavailable";
        }
    }
}
