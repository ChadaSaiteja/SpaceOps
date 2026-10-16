using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using StorageIntelligence.Native;
using StorageIntelligence.Services;

namespace StorageIntelligence.Pages;

/// <summary>Applications destination: installed-app storage attribution, uninstall, and orphaned leftovers (ADR-014).</summary>
public sealed partial class ApplicationsPage : Page
{
    private readonly AppSession _session = AppSession.Current;
    private List<InstalledAppViewModel> _allApps = new();
    private List<AppLeftoverViewModel> _allLeftovers = new();
    private bool _showingLeftovers;
    private bool _loaded;

    public ApplicationsPage()
    {
        InitializeComponent();
        NavigationCacheMode = NavigationCacheMode.Required;
        _session.TreeChanged += () => _loaded = false;
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (!_loaded)
        {
            if (_showingLeftovers)
            {
                await LoadLeftoversAsync();
            }
            else
            {
                await LoadApplicationsAsync();
            }
        }
    }

    private async Task LoadApplicationsAsync()
    {
        AppsSummaryText.Text = "Discovering installed applications...";
        RefreshAppsButton.IsEnabled = false;

        try
        {
            var apps = await AppManagerService.DiscoverApplicationsAsync(_session.Tree);
            _allApps = apps.Select(a => new InstalledAppViewModel(a)).ToList();
            ApplyAppFilters();
            _loaded = true;

            ulong totalSize = 0;
            foreach (var a in apps) totalSize += a.DisplaySizeBytes;
            AppsSummaryText.Text = $"{_allApps.Count:N0} applications installed • {Fmt.Bytes(totalSize)} total storage";
        }
        catch (Exception ex)
        {
            AppsSummaryText.Text = $"Failed to discover applications: {ex.Message}";
        }
        finally
        {
            RefreshAppsButton.IsEnabled = true;
        }
    }

    private void AppFilterTextBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        ApplyAppFilters();
    }

    private void AppTypeFilterCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        ApplyAppFilters();
    }

    private void ApplyAppFilters()
    {
        if (_showingLeftovers) return;

        string filter = AppFilterTextBox?.Text?.Trim() ?? "";
        int typeIndex = AppTypeFilterCombo?.SelectedIndex ?? 0;

        var filtered = _allApps.AsEnumerable();

        if (!string.IsNullOrEmpty(filter))
        {
            filtered = filtered.Where(a =>
                a.Name.Contains(filter, StringComparison.OrdinalIgnoreCase) ||
                a.Publisher.Contains(filter, StringComparison.OrdinalIgnoreCase) ||
                a.InstallLocation.Contains(filter, StringComparison.OrdinalIgnoreCase));
        }

        if (typeIndex == 1) // Desktop Win32
        {
            filtered = filtered.Where(a => a.Kind == AppKind.Win32);
        }
        else if (typeIndex == 2) // Store MSIX
        {
            filtered = filtered.Where(a => a.Kind == AppKind.Msix);
        }

        InstalledAppsListView.ItemsSource = filtered.ToList();
    }

    private async void RefreshApps_Click(object sender, RoutedEventArgs e)
    {
        if (_showingLeftovers)
        {
            await LoadLeftoversAsync();
        }
        else
        {
            await LoadApplicationsAsync();
        }
    }

    private async void ToggleLeftovers_Click(object sender, RoutedEventArgs e)
    {
        _showingLeftovers = !_showingLeftovers;
        if (_showingLeftovers)
        {
            InstalledAppsListView.Visibility = Visibility.Collapsed;
            LeftoversListView.Visibility = Visibility.Visible;
            ToggleLeftoversText.Text = "View Applications";
            AppFilterTextBox.IsEnabled = false;
            AppTypeFilterCombo.IsEnabled = false;
            await LoadLeftoversAsync();
        }
        else
        {
            LeftoversListView.Visibility = Visibility.Collapsed;
            InstalledAppsListView.Visibility = Visibility.Visible;
            ToggleLeftoversText.Text = "View Leftovers";
            AppFilterTextBox.IsEnabled = true;
            AppTypeFilterCombo.IsEnabled = true;
            ApplyAppFilters();
        }
    }

    private async Task LoadLeftoversAsync()
    {
        AppsSummaryText.Text = "Scanning AppData and ProgramData for orphaned leftovers...";
        RefreshAppsButton.IsEnabled = false;

        try
        {
            var apps = _allApps.Select(vm => vm.Info).ToList();
            var leftovers = await AppManagerService.DiscoverLeftoversAsync(apps, _session.Tree);
            _allLeftovers = leftovers.Select(l => new AppLeftoverViewModel(l)).ToList();
            LeftoversListView.ItemsSource = _allLeftovers;
            _loaded = true;

            ulong totalLeftoverBytes = 0;
            foreach (var l in leftovers) totalLeftoverBytes += l.SizeBytes;

            AppsSummaryText.Text = $"{_allLeftovers.Count:N0} orphaned folders found • {Fmt.Bytes(totalLeftoverBytes)} reclaimable";
        }
        catch (Exception ex)
        {
            AppsSummaryText.Text = $"Failed to scan leftovers: {ex.Message}";
        }
        finally
        {
            RefreshAppsButton.IsEnabled = true;
        }
    }

    private void AppOpenLocation_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is InstalledAppViewModel app)
        {
            Fmt.RevealInExplorer(app.InstallLocation);
        }
    }

    private async void AppUninstall_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is InstalledAppViewModel app)
        {
            var dialog = new ContentDialog
            {
                Title = "Uninstall Application?",
                Content = $"Are you sure you want to uninstall {app.Name}?\n\nThis will invoke the official vendor uninstaller.",
                PrimaryButtonText = "Launch Uninstaller",
                CloseButtonText = "Cancel",
                XamlRoot = this.XamlRoot
            };

            var res = await dialog.ShowAsync();
            if (res == ContentDialogResult.Primary)
            {
                try
                {
                    await AppManagerService.UninstallApplicationAsync(app.Info);
                    AppsSummaryText.Text = $"Launched uninstaller for {app.Name}.";
                }
                catch (Exception ex)
                {
                    AppsSummaryText.Text = $"Failed to launch uninstaller: {ex.Message}";
                }
            }
        }
    }

    private void CleanLeftover_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is AppLeftoverViewModel leftover)
        {
            try
            {
                if (Directory.Exists(leftover.Path))
                {
                    Microsoft.VisualBasic.FileIO.FileSystem.DeleteDirectory(
                        leftover.Path,
                        Microsoft.VisualBasic.FileIO.UIOption.OnlyErrorDialogs,
                        Microsoft.VisualBasic.FileIO.RecycleOption.SendToRecycleBin);

                    leftover.IsCleaned = true;
                    AppsSummaryText.Text = $"Moved {leftover.AppName} leftover to Recycle Bin ({leftover.FormattedSize} reclaimed).";
                }
            }
            catch (Exception ex)
            {
                AppsSummaryText.Text = $"Failed to clean leftover: {ex.Message}";
            }
        }
    }
}
