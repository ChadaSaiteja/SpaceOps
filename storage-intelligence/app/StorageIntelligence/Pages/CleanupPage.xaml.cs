using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using StorageIntelligence.Services;

namespace StorageIntelligence.Pages;

/// <summary>Cleanup destination: detect safe-to-clean candidates and execute selected rules (ADR-013).</summary>
public sealed partial class CleanupPage : Page
{
    private readonly AppSession _session = AppSession.Current;
    private List<CleanupCandidateViewModel> _cleanupCandidates = new();

    public CleanupPage()
    {
        InitializeComponent();
        NavigationCacheMode = NavigationCacheMode.Required;
        _session.TreeChanged += () => { _cleanupCandidates = new(); CleanupCandidatesList.ItemsSource = null; };
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (_session.Tree != null)
        {
            AnalyzeCleanup();
        }
        else
        {
            CleanupTotalText.Text = "Please perform a scan first to analyze storage candidates.";
        }
    }

    private void AnalyzeCleanup_Click(object sender, RoutedEventArgs e)
    {
        AnalyzeCleanup();
    }

    private async void AnalyzeCleanup()
    {
        var tree = _session.Tree;
        if (tree == null) return;

        CleanupTotalText.Text = "Analyzing cleanup candidates...";
        CleanupStatusText.Text = "Scanning candidate paths...";

        try
        {
            var sw = Stopwatch.StartNew();
            var candidates = await tree.DetectCleanupCandidatesAsync();
            sw.Stop();

            _cleanupCandidates = candidates.Select(c => new CleanupCandidateViewModel(c)).ToList();
            CleanupCandidatesList.ItemsSource = _cleanupCandidates;

            ulong totalReclaimable = 0;
            foreach (var c in _cleanupCandidates)
            {
                totalReclaimable += c.TotalBytes;
            }

            CleanupTotalText.Text = $"Total Reclaimable: {Fmt.Bytes(totalReclaimable)} across {candidates.Length} categories";
            CleanupStatusText.Text = $"Analysis complete in {sw.ElapsedMilliseconds} ms. Protected paths verified.";
        }
        catch (Exception ex)
        {
            CleanupStatusText.Text = $"Analysis error: {ex.Message}";
        }
    }

    private async void ExecuteCleanup_Click(object sender, RoutedEventArgs e)
    {
        var tree = _session.Tree;
        if (tree == null || _cleanupCandidates.Count == 0) return;

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
                var report = await tree.ExecuteCleanupRuleAsync(item.RuleId, dryRun, sendToRecycleBin);
                totalFilesReclaimed += report.FilesReclaimed;
                totalBytesReclaimed += report.BytesReclaimed;
                totalFilesFailed += report.FilesFailed;
            }

            string mode = dryRun ? "Simulated" : (sendToRecycleBin ? "Moved to Recycle Bin" : "Deleted");
            string status = $"{mode}: {totalFilesReclaimed:N0} files ({Fmt.Bytes(totalBytesReclaimed)})";
            if (totalFilesFailed > 0)
            {
                status += $" • {totalFilesFailed:N0} locked/protected files skipped";
            }
            CleanupStatusText.Text = status;

            if (!dryRun)
            {
                _session.NotifyReclaimableChanged();
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
}
