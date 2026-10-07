using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using StorageIntelligence.Controls;
using StorageIntelligence.Native;

namespace StorageIntelligence;

public sealed partial class MainPage : Page
{
    private CancellationTokenSource? _scanCts;
    private CancellationTokenSource? _searchCts;
    private StorageTreeService? _currentTree;
    private NodeInfo? _selectedNode;
    private IReadOnlyList<BreadcrumbItem> _currentBreadcrumbs = Array.Empty<BreadcrumbItem>();
    private List<CleanupCandidateViewModel> _cleanupCandidates = new();
    private List<InstalledAppViewModel> _allApps = new();
    private List<AppLeftoverViewModel> _allLeftovers = new();
    private List<DevArtifactViewModel> _allDevArtifacts = new();
    private bool _showingLeftovers;

    public MainPage()
    {
        InitializeComponent();

        // Default path to user profile or Windows drive
        string defaultPath = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (string.IsNullOrEmpty(defaultPath) || !Directory.Exists(defaultPath))
        {
            defaultPath = "C:\\";
        }
        PathTextBox.Text = defaultPath;
    }

    private async void ScanButton_Click(object sender, RoutedEventArgs e)
    {
        string path = PathTextBox.Text.Trim();
        if (string.IsNullOrEmpty(path)) return;

        _scanCts = new CancellationTokenSource();
        ScanButton.IsEnabled = false;
        CancelButton.IsEnabled = true;
        ScanProgressBar.Visibility = Visibility.Visible;
        StatusTextBlock.Text = $"Starting scan of {path}...";
        MetricsTextBlock.Text = string.Empty;

        var progress = new Progress<ScanProgressInfo>(p =>
        {
            StatusTextBlock.Text = $"Scanning: {p.FilesScanned:N0} files  •  {FormatBytes(p.BytesScanned)}";
        });

        try
        {
            var sw = Stopwatch.StartNew();
            using var scanResult = await ScanService.ScanDriveWithResultAsync(path, _scanCts.Token, progress);
            sw.Stop();

            StatusTextBlock.Text = $"Building in-memory storage tree...";

            var treeSw = Stopwatch.StartNew();
            var newTree = await StorageTreeService.BuildAsync(scanResult);
            treeSw.Stop();

            // Dispose previous tree
            _currentTree?.Dispose();
            _currentTree = newTree;

            Treemap.SetTree(_currentTree);

            ulong nodeCount = _currentTree.GetNodeCount();
            var rootInfo = _currentTree.GetNodeInfo(_currentTree.GetRootId());

            StatusTextBlock.Text = $"Ready • {nodeCount:N0} nodes scanned in {sw.Elapsed.TotalSeconds:F2}s (tree built in {treeSw.ElapsedMilliseconds}ms)";
            MetricsTextBlock.Text = $"Total Size: {FormatBytes(rootInfo.Size)}";

            // Asynchronously persist to SQLite index for instant future launches (ADR-016)
            _ = Task.Run(async () =>
            {
                try
                {
                    await IndexService.SaveTreeAsync(newTree, path);
                }
                catch
                {
                    // Non-fatal background indexing
                }
            });
        }
        catch (ScanException sex) when (sex.ErrorCode == 2)
        {
            StatusTextBlock.Text = "Scan cancelled by user.";
        }
        catch (Exception ex)
        {
            StatusTextBlock.Text = $"Error: {ex.Message}";
        }
        finally
        {
            ScanButton.IsEnabled = true;
            CancelButton.IsEnabled = false;
            ScanProgressBar.Visibility = Visibility.Collapsed;
            _scanCts = null;
        }
    }

    private void CancelButton_Click(object sender, RoutedEventArgs e)
    {
        _scanCts?.Cancel();
        CancelButton.IsEnabled = false;
        StatusTextBlock.Text = "Cancelling scan...";
    }

    private void Treemap_DrillDownChanged(object? sender, IReadOnlyList<BreadcrumbItem> breadcrumbs)
    {
        _currentBreadcrumbs = breadcrumbs;
        Breadcrumbs.ItemsSource = breadcrumbs.Select(b => b.Name).ToList();
        UpButton.IsEnabled = breadcrumbs.Count > 1;
    }

    private void Breadcrumbs_ItemClicked(BreadcrumbBar sender, BreadcrumbBarItemClickedEventArgs args)
    {
        if (args.Index >= 0 && args.Index < _currentBreadcrumbs.Count)
        {
            ulong targetId = _currentBreadcrumbs[args.Index].NodeId;
            Treemap.NavigateTo(targetId);
        }
    }

    private void UpButton_Click(object sender, RoutedEventArgs e)
    {
        Treemap.NavigateUp();
    }

    private void Treemap_NodeSelected(object? sender, NodeInfo info)
    {
        _selectedNode = info;

        DetailTitle.Text = info.Name;
        DetailPath.Text = info.FullPath ?? string.Empty;
        DetailSize.Text = FormatBytes(info.Size);

        if (info.Kind == NodeKind.Directory)
        {
            DetailIcon.Glyph = "\uE8B7"; // Folder icon
            DetailCategory.Text = "Folder";
            DirStatsGrid.Visibility = Visibility.Visible;
            DetailFileCount.Text = $"{info.FileCount:N0}";
            DetailDirCount.Text = $"{info.DirCount:N0}";
            DrillDownButton.IsEnabled = true;
        }
        else
        {
            DetailIcon.Glyph = "\uE8A5"; // File icon
            string ext = Path.GetExtension(info.Name).TrimStart('.');
            var cat = TreemapService.ClassifyExtension(ext);
            DetailCategory.Text = $"{cat} ({ext.ToUpperInvariant()})";
            DirStatsGrid.Visibility = Visibility.Collapsed;
            DrillDownButton.IsEnabled = false;
        }

        OpenExplorerButton.IsEnabled = !string.IsNullOrEmpty(info.FullPath);
    }

    private void DrillDownButton_Click(object sender, RoutedEventArgs e)
    {
        if (_selectedNode.HasValue && _selectedNode.Value.Kind == NodeKind.Directory)
        {
            Treemap.DrillDown(_selectedNode.Value.Id);
        }
    }

    private void OpenExplorerButton_Click(object sender, RoutedEventArgs e)
    {
        if (!_selectedNode.HasValue || string.IsNullOrEmpty(_selectedNode.Value.FullPath)) return;

        string path = _selectedNode.Value.FullPath;
        try
        {
            if (File.Exists(path))
            {
                Process.Start(new ProcessStartInfo("explorer.exe", $"/select,\"{path}\"") { UseShellExecute = true });
            }
            else if (Directory.Exists(path))
            {
                Process.Start(new ProcessStartInfo("explorer.exe", $"\"{path}\"") { UseShellExecute = true });
            }
        }
        catch (Exception ex)
        {
            StatusTextBlock.Text = $"Failed to open explorer: {ex.Message}";
        }
    }

    private async void SearchBox_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        if (args.Reason != AutoSuggestionBoxTextChangeReason.UserInput) return;

        string query = sender.Text.Trim();
        if (_currentTree == null || string.IsNullOrWhiteSpace(query))
        {
            sender.ItemsSource = null;
            return;
        }

        _searchCts?.Cancel();
        _searchCts = new CancellationTokenSource();
        var token = _searchCts.Token;

        try
        {
            // Debounce keystrokes by 100ms
            await Task.Delay(100, token);
            if (token.IsCancellationRequested) return;

            var sw = Stopwatch.StartNew();
            var results = await _currentTree.SearchAsync(query, maxResults: 30);
            sw.Stop();

            if (token.IsCancellationRequested) return;

            var viewModels = results.Select(r => new SearchResultViewModel(r)).ToList();
            sender.ItemsSource = viewModels;

            if (results.Length > 0)
            {
                StatusTextBlock.Text = $"Found {results.Length} matching items in {sw.ElapsedMilliseconds} ms for '{query}'";
            }
        }
        catch (OperationCanceledException)
        {
            // Ignore debounced cancellations
        }
        catch (Exception ex)
        {
            StatusTextBlock.Text = $"Search error: {ex.Message}";
        }
    }

    private void SearchBox_SuggestionChosen(AutoSuggestBox sender, AutoSuggestBoxSuggestionChosenEventArgs args)
    {
        if (args.SelectedItem is SearchResultViewModel chosen)
        {
            Treemap.NavigateAndSelect(chosen.NodeId);
        }
    }

    private async void SearchBox_QuerySubmitted(AutoSuggestBox sender, AutoSuggestBoxQuerySubmittedEventArgs args)
    {
        if (args.ChosenSuggestion is SearchResultViewModel chosen)
        {
            Treemap.NavigateAndSelect(chosen.NodeId);
            return;
        }

        string query = args.QueryText?.Trim() ?? string.Empty;
        if (_currentTree == null || string.IsNullOrWhiteSpace(query)) return;

        try
        {
            var results = await _currentTree.SearchAsync(query, maxResults: 10);
            if (results.Length > 0)
            {
                Treemap.NavigateAndSelect(results[0].NodeId);
            }
            else
            {
                StatusTextBlock.Text = $"No files or folders found matching '{query}'";
            }
        }
        catch (Exception ex)
        {
            StatusTextBlock.Text = $"Search failed: {ex.Message}";
        }
    }

    private void CleanupButton_Click(object sender, RoutedEventArgs e)
    {
        CleanupOverlay.Visibility = Visibility.Visible;
        if (_currentTree != null)
        {
            AnalyzeCleanup();
        }
        else
        {
            CleanupTotalText.Text = "Please perform a scan first to analyze storage candidates.";
        }
    }

    private void CloseCleanup_Click(object sender, RoutedEventArgs e)
    {
        CleanupOverlay.Visibility = Visibility.Collapsed;
    }

    private void AnalyzeCleanup_Click(object sender, RoutedEventArgs e)
    {
        AnalyzeCleanup();
    }

    private async void AnalyzeCleanup()
    {
        if (_currentTree == null) return;

        CleanupTotalText.Text = "Analyzing cleanup candidates...";
        CleanupStatusText.Text = "Scanning candidate paths...";

        try
        {
            var sw = Stopwatch.StartNew();
            var candidates = await _currentTree.DetectCleanupCandidatesAsync();
            sw.Stop();

            _cleanupCandidates = candidates.Select(c => new CleanupCandidateViewModel(c)).ToList();
            CleanupCandidatesList.ItemsSource = _cleanupCandidates;

            ulong totalReclaimable = 0;
            foreach (var c in _cleanupCandidates)
            {
                totalReclaimable += c.TotalBytes;
            }

            CleanupTotalText.Text = $"Total Reclaimable: {FormatBytes(totalReclaimable)} across {candidates.Length} categories";
            CleanupStatusText.Text = $"Analysis complete in {sw.ElapsedMilliseconds} ms. Protected paths verified.";
        }
        catch (Exception ex)
        {
            CleanupStatusText.Text = $"Analysis error: {ex.Message}";
        }
    }

    private async void ExecuteCleanup_Click(object sender, RoutedEventArgs e)
    {
        if (_currentTree == null || _cleanupCandidates.Count == 0) return;

        var selected = _cleanupCandidates.Where(c => c.IsSelected && !c.IsProtected).ToList();
        if (selected.Count == 0)
        {
            CleanupStatusText.Text = "No categories selected for cleanup.";
            return;
        }

        bool dryRun = DryRunCheckBox.IsChecked ?? false;
        bool sendToRecycleBin = RecycleBinCheckBox.IsChecked ?? true;

        ExecuteCleanupButton.IsEnabled = false;
        CleanupStatusText.Text = dryRun ? "Simulating cleanup (dry run)..." : "Executing safe cleanup...";

        try
        {
            ulong totalFilesReclaimed = 0;
            ulong totalBytesReclaimed = 0;
            ulong totalFilesFailed = 0;

            foreach (var item in selected)
            {
                var report = await _currentTree.ExecuteCleanupRuleAsync(item.RuleId, dryRun, sendToRecycleBin);
                totalFilesReclaimed += report.FilesReclaimed;
                totalBytesReclaimed += report.BytesReclaimed;
                totalFilesFailed += report.FilesFailed;
            }

            string mode = dryRun ? "Simulated" : (sendToRecycleBin ? "Moved to Recycle Bin" : "Deleted");
            string status = $"{mode}: {totalFilesReclaimed:N0} files ({FormatBytes(totalBytesReclaimed)})";
            if (totalFilesFailed > 0)
            {
                status += $" • {totalFilesFailed:N0} locked/protected files skipped";
            }
            CleanupStatusText.Text = status;

            if (!dryRun)
            {
                AnalyzeCleanup();
            }
        }
        catch (Exception ex)
        {
            CleanupStatusText.Text = $"Cleanup error: {ex.Message}";
        }
        finally
        {
            ExecuteCleanupButton.IsEnabled = true;
        }
    }

    private void DepthComboBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        // Reserved for dynamic depth adjustment in future revisions
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

    // -----------------------------------------------------------------------
    // Phase 7: Application Management & Leftovers (ADR-014)
    // -----------------------------------------------------------------------

    private async void AppsButton_Click(object sender, RoutedEventArgs e)
    {
        AppsOverlay.Visibility = Visibility.Visible;
        await LoadApplicationsAsync();
    }

    private void CloseApps_Click(object sender, RoutedEventArgs e)
    {
        AppsOverlay.Visibility = Visibility.Collapsed;
    }

    private async Task LoadApplicationsAsync()
    {
        AppsSummaryText.Text = "Discovering installed applications...";
        RefreshAppsButton.IsEnabled = false;

        try
        {
            var apps = await AppManagerService.DiscoverApplicationsAsync(_currentTree);
            _allApps = apps.Select(a => new InstalledAppViewModel(a)).ToList();
            ApplyAppFilters();

            ulong totalSize = 0;
            foreach (var a in apps) totalSize += a.DisplaySizeBytes;
            AppsSummaryText.Text = $"{_allApps.Count:N0} applications installed • {FormatBytes(totalSize)} total storage";
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
            AppsScrollViewer.Visibility = Visibility.Collapsed;
            LeftoversScrollViewer.Visibility = Visibility.Visible;
            ToggleLeftoversText.Text = "View Applications";
            AppFilterTextBox.IsEnabled = false;
            AppTypeFilterCombo.IsEnabled = false;
            await LoadLeftoversAsync();
        }
        else
        {
            LeftoversScrollViewer.Visibility = Visibility.Collapsed;
            AppsScrollViewer.Visibility = Visibility.Visible;
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
            var leftovers = await AppManagerService.DiscoverLeftoversAsync(apps, _currentTree);
            _allLeftovers = leftovers.Select(l => new AppLeftoverViewModel(l)).ToList();
            LeftoversListView.ItemsSource = _allLeftovers;

            ulong totalLeftoverBytes = 0;
            foreach (var l in leftovers) totalLeftoverBytes += l.SizeBytes;

            AppsSummaryText.Text = $"{_allLeftovers.Count:N0} orphaned folders found • {FormatBytes(totalLeftoverBytes)} reclaimable";
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
            if (Directory.Exists(app.InstallLocation))
            {
                Process.Start("explorer.exe", app.InstallLocation);
            }
            else if (File.Exists(app.InstallLocation))
            {
                Process.Start("explorer.exe", $"/select,\"{app.InstallLocation}\"");
            }
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

    // ---------------------------------------------------------------------------
    // Phase 8: Developer Storage Intelligence & Safe Reclamation Handlers
    // ---------------------------------------------------------------------------

    private async void DevStorageButton_Click(object sender, RoutedEventArgs e)
    {
        DevStorageOverlay.Visibility = Visibility.Visible;
        await LoadDevArtifactsAsync();
    }

    private void CloseDevStorage_Click(object sender, RoutedEventArgs e)
    {
        DevStorageOverlay.Visibility = Visibility.Collapsed;
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
            var artifacts = await DevStorageService.DiscoverArtifactsAsync(_currentTree);
            _allDevArtifacts = artifacts.Select(a => new DevArtifactViewModel(a)).ToList();
            ApplyDevFilters();

            ulong totalBytes = 0;
            ulong dormantBytes = 0;
            foreach (var a in artifacts)
            {
                totalBytes += a.SizeBytes;
                if (a.IsDormant) dormantBytes += a.SizeBytes;
            }

            DevStorageSummaryText.Text = $"{_allDevArtifacts.Count:N0} developer artifacts found • {FormatBytes(totalBytes)} total ({FormatBytes(dormantBytes)} dormant)";
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
            if (Directory.Exists(art.Path))
            {
                Process.Start("explorer.exe", art.Path);
            }
            else if (File.Exists(art.Path))
            {
                Process.Start("explorer.exe", $"/select,\"{art.Path}\"");
            }
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
                    DevStorageSummaryText.Text = $"Reclaimed {FormatBytes(report.BytesReclaimed)} ({report.FilesReclaimed} files moved to Recycle Bin).";
                    await LoadDevArtifactsAsync();
                }
                catch (Exception ex)
                {
                    DevStorageSummaryText.Text = $"Failed to reclaim artifact: {ex.Message}";
                }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Phase 9: Incremental Indexing & SQLite Synchronization Handlers (ADR-016)
    // ---------------------------------------------------------------------------

    private async void SyncButton_Click(object sender, RoutedEventArgs e)
    {
        string path = PathTextBox.Text.Trim();
        if (string.IsNullOrEmpty(path)) return;

        SyncButton.IsEnabled = false;

        try
        {
            if (_currentTree != null)
            {
                // Incremental synchronization via USN Journal or timestamp diff
                StatusTextBlock.Text = $"Incrementally synchronizing {path} via USN / timestamp diff...";
                var report = await IndexService.SyncTreeAsync(_currentTree, path);

                Treemap.RecomputeLayout();
                var rootInfo = _currentTree.GetNodeInfo(_currentTree.GetRootId());
                StatusTextBlock.Text = $"Synced in {report.SyncDurationMs}ms • {report.NodesUpdated} updated, {report.NodesRemoved} removed ({report.FormattedBytesDelta})";
                MetricsTextBlock.Text = $"Total Size: {FormatBytes(rootInfo.Size)}";
            }
            else
            {
                // Instant hydration from local SQLite index (< 100ms)
                StatusTextBlock.Text = $"Checking SQLite index for {path}...";
                var cachedTree = await IndexService.LoadTreeAsync(path);

                if (cachedTree != null)
                {
                    _currentTree?.Dispose();
                    _currentTree = cachedTree;
                    Treemap.SetTree(_currentTree);

                    ulong nodeCount = _currentTree.GetNodeCount();
                    var rootInfo = _currentTree.GetNodeInfo(_currentTree.GetRootId());

                    StatusTextBlock.Text = $"Loaded from SQLite index in < 100ms ({nodeCount:N0} nodes)";
                    MetricsTextBlock.Text = $"Total Size: {FormatBytes(rootInfo.Size)}";
                }
                else
                {
                    StatusTextBlock.Text = $"No cached index found for {path}. Click 'Scan & Visualize' to perform initial scan.";
                }
            }
        }
        catch (Exception ex)
        {
            StatusTextBlock.Text = $"Index Sync Error: {ex.Message}";
        }
        finally
        {
            SyncButton.IsEnabled = true;
        }
    }
}
