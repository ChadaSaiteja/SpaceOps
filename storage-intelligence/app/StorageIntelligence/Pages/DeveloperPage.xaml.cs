using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using StorageIntelligence.Native;
using StorageIntelligence.Services;

namespace StorageIntelligence.Pages;

/// <summary>Developer destination: dev-artifact discovery (node_modules, target, caches, Docker/WSL) and 1-click reclamation (ADR-015).</summary>
public sealed partial class DeveloperPage : Page
{
    private readonly AppSession _session = AppSession.Current;
    private List<DevArtifactViewModel> _allDevArtifacts = new();
    private bool _loaded;

    public DeveloperPage()
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
            await LoadDevArtifactsAsync();
        }
    }

    private async void RefreshDevStorage_Click(object sender, RoutedEventArgs e)
    {
        await LoadDevArtifactsAsync();
    }

    private async Task LoadDevArtifactsAsync()
    {
        DevStorageSummaryText.Text = "Scanning workspace and canonical developer caches...";
        RefreshDevStorageButton.IsEnabled = false;

        try
        {
            var artifacts = await DevStorageService.DiscoverArtifactsAsync(_session.Tree);
            _allDevArtifacts = artifacts.Select(a => new DevArtifactViewModel(a)).ToList();
            ApplyDevFilters();
            _loaded = true;

            ulong totalBytes = 0;
            ulong dormantBytes = 0;
            foreach (var a in artifacts)
            {
                totalBytes += a.SizeBytes;
                if (a.IsDormant) dormantBytes += a.SizeBytes;
            }

            DevStorageSummaryText.Text = $"{_allDevArtifacts.Count:N0} developer artifacts found • {Fmt.Bytes(totalBytes)} total ({Fmt.Bytes(dormantBytes)} dormant)";
        }
        catch (Exception ex)
        {
            DevStorageSummaryText.Text = $"Failed to analyze developer storage: {ex.Message}";
        }
        finally
        {
            RefreshDevStorageButton.IsEnabled = true;
        }
    }

    private void DevFilterTextBox_TextChanged(object sender, TextChangedEventArgs e)
    {
        ApplyDevFilters();
    }

    private void DevEcosystemFilterCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        ApplyDevFilters();
    }

    private void DevDormantOnlyCheck_Changed(object sender, RoutedEventArgs e)
    {
        ApplyDevFilters();
    }

    private void ApplyDevFilters()
    {
        string filter = DevFilterTextBox?.Text?.Trim() ?? "";
        int ecoIndex = DevEcosystemFilterCombo?.SelectedIndex ?? 0;
        bool dormantOnly = DevDormantOnlyCheck?.IsChecked == true;

        var filtered = _allDevArtifacts.AsEnumerable();

        if (!string.IsNullOrEmpty(filter))
        {
            filtered = filtered.Where(a =>
                a.Name.Contains(filter, StringComparison.OrdinalIgnoreCase) ||
                a.ProjectName.Contains(filter, StringComparison.OrdinalIgnoreCase) ||
                a.Path.Contains(filter, StringComparison.OrdinalIgnoreCase));
        }

        if (ecoIndex > 0)
        {
            // 1: Node, 2: Rust, 3: .NET, 4: Python, 5: Java, 6: DockerWsl, 7: Git
            var targetEco = ecoIndex switch
            {
                1 => DevEcosystem.NodeJs,
                2 => DevEcosystem.Rust,
                3 => DevEcosystem.DotNet,
                4 => DevEcosystem.Python,
                5 => DevEcosystem.Java,
                6 => DevEcosystem.DockerWsl,
                7 => DevEcosystem.Git,
                _ => (DevEcosystem?)null,
            };

            if (targetEco.HasValue)
            {
                filtered = filtered.Where(a => a.Info.Ecosystem == targetEco.Value);
            }
        }

        if (dormantOnly)
        {
            filtered = filtered.Where(a => a.IsDormant);
        }

        DevArtifactsListView.ItemsSource = filtered.ToList();
    }

    private void DevOpenLocation_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is DevArtifactViewModel art)
        {
            Fmt.RevealInExplorer(art.Path);
        }
    }

    private async void DevCleanArtifact_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button btn && btn.Tag is DevArtifactViewModel art)
        {
            var dialog = new ContentDialog
            {
                Title = "Reclaim Developer Storage?",
                Content = $"Are you sure you want to clean '{art.Name}' for {art.ProjectName}?\n\nPath: {art.Path}\nReclaimable: {art.FormattedSize}\n\nFiles will be safely moved to the Windows Recycle Bin so they can be restored if needed.",
                PrimaryButtonText = "Move to Recycle Bin",
                CloseButtonText = "Cancel",
                XamlRoot = this.XamlRoot,
            };

            var res = await dialog.ShowAsync();
            if (res == ContentDialogResult.Primary)
            {
                try
                {
                    var report = await DevStorageService.CleanArtifactAsync(art.Path, dryRun: false, sendToRecycleBin: true);
                    DevStorageSummaryText.Text = $"Reclaimed {Fmt.Bytes(report.BytesReclaimed)} ({report.FilesReclaimed} files moved to Recycle Bin).";
                    await LoadDevArtifactsAsync();
                }
                catch (Exception ex)
                {
                    DevStorageSummaryText.Text = $"Failed to reclaim artifact: {ex.Message}";
                }
            }
        }
    }
}
