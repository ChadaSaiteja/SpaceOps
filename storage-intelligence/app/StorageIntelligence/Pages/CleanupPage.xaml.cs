using System;
using System.Diagnostics;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using StorageIntelligence.Native;
using StorageIntelligence.Services;

namespace StorageIntelligence.Pages;

/// <summary>
/// Cleanup destination: one candidate list and one reclaimable total across system junk, dormant developer
/// artifacts and orphaned app leftovers, with dry-run and Recycle Bin options (ADR-013).
/// </summary>
public sealed partial class CleanupPage : Page
{
    private readonly AppSession _session = AppSession.Current;
    private ReclaimableSet _set = ReclaimableSet.Empty;
    private bool _loaded;
    private bool _ready;

    public CleanupPage()
    {
        InitializeComponent();
        NavigationCacheMode = NavigationCacheMode.Required;
        _ready = true;

        _session.TreeChanged += () => { _loaded = false; _set = ReclaimableSet.Empty; ApplyFilter(); };
        _session.ReclaimableInvalidated += () => _loaded = false;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        if (_session.Tree == null)
        {
            CleanupTotalText.Text = "Please perform a scan first to analyze storage candidates.";
            CleanupBreakdownText.Text = string.Empty;
        }
        else if (!_loaded)
        {
            AnalyzeCleanup(force: false);
        }
    }

    private void AnalyzeCleanup_Click(object sender, RoutedEventArgs e)
    {
        AnalyzeCleanup(force: true);
    }

    private async void AnalyzeCleanup(bool force)
    {
        var tree = _session.Tree;
        if (tree == null) return;

        CleanupTotalText.Text = "Analyzing cleanup candidates...";
        CleanupStatusText.Text = "Scanning system junk, developer caches and application leftovers...";
        AnalyzeRing.IsActive = true;
        AnalyzeButton.IsEnabled = false;

        try
        {
            var sw = Stopwatch.StartNew();
            var set = await _session.GetReclaimableAsync(force);
            sw.Stop();
            if (!ReferenceEquals(tree, _session.Tree)) return;

            _set = set;
            _loaded = true;
            ApplyFilter();
            UpdateTotals();

            CleanupStatusText.Text = set.Errors.Count == 0
                ? $"Analysis complete in {sw.ElapsedMilliseconds} ms. Protected paths verified."
                : string.Join("  •  ", set.Errors);
        }
        catch (Exception ex)
        {
            CleanupStatusText.Text = $"Analysis error: {ex.Message}";
        }
        finally
        {
            AnalyzeRing.IsActive = false;
            AnalyzeButton.IsEnabled = true;
        }
    }

    private void UpdateTotals()
    {
        CleanupTotalText.Text = $"Total Reclaimable: {Fmt.Bytes(_set.TotalBytes)} across {_set.Reclaimable.Count()} items";
        CleanupBreakdownText.Text =
            $"System junk {Fmt.Bytes(_set.BytesFor(CandidateSource.SystemJunk))} ({_set.CountFor(CandidateSource.SystemJunk)})   •   " +
            $"Developer artifacts {Fmt.Bytes(_set.BytesFor(CandidateSource.DevArtifact))} ({_set.CountFor(CandidateSource.DevArtifact)})   •   " +
            $"App leftovers {Fmt.Bytes(_set.BytesFor(CandidateSource.AppLeftover))} ({_set.CountFor(CandidateSource.AppLeftover)})";
        UpdateSelectedTotal();
    }

    private void UpdateSelectedTotal()
    {
        var picked = _set.Candidates.Where(c => c.IsSelected && !c.IsProtected).ToList();
        ulong bytes = picked.Aggregate(0UL, (sum, c) => sum + c.Bytes);
        SelectedTotalText.Text = picked.Count == 0
            ? "Nothing selected"
            : $"Selected: {picked.Count} items • {Fmt.Bytes(bytes)}";
    }

    private void ApplyFilter()
    {
        if (!_ready) return;

        var items = SourceFilterCombo.SelectedIndex switch
        {
            1 => _set.Candidates.Where(c => c.Source == CandidateSource.SystemJunk),
            2 => _set.Candidates.Where(c => c.Source == CandidateSource.DevArtifact),
            3 => _set.Candidates.Where(c => c.Source == CandidateSource.AppLeftover),
            _ => _set.Candidates.AsEnumerable(),
        };
        CleanupCandidatesList.ItemsSource = items.ToList();
        UpdateSelectedTotal();
    }

    private void SourceFilterCombo_SelectionChanged(object sender, SelectionChangedEventArgs e) => ApplyFilter();

    private void Candidate_SelectionChanged(object sender, RoutedEventArgs e)
    {
        if (_ready) UpdateSelectedTotal();
    }

    private void SelectSafe_Click(object sender, RoutedEventArgs e)
    {
        foreach (var c in _set.Candidates)
        {
            c.IsSelected = !c.IsProtected && c.Risk == RiskLevel.Low && c.Bytes > 0;
        }
        UpdateSelectedTotal();
    }

    private void ClearSelection_Click(object sender, RoutedEventArgs e)
    {
        foreach (var c in _set.Candidates) c.IsSelected = false;
        UpdateSelectedTotal();
    }

    private async void ExecuteCleanup_Click(object sender, RoutedEventArgs e)
    {
        var tree = _session.Tree;
        if (tree == null || _set.Candidates.Count == 0) return;

        var selected = _set.Candidates.Where(c => c.IsSelected && !c.IsProtected).ToList();
        if (selected.Count == 0)
        {
            CleanupStatusText.Text = "No items selected for cleanup.";
            return;
        }

        bool dryRun = DryRunCheckBox.IsChecked ?? false;
        bool sendToRecycleBin = RecycleBinCheckBox.IsChecked ?? true;

        ExecuteCleanupButton.IsEnabled = false;
        CleanupStatusText.Text = dryRun ? "Simulating cleanup (dry run)..." : "Executing safe cleanup...";

        try
        {
            var report = await ReclaimableService.ExecuteAsync(tree, selected, dryRun, sendToRecycleBin);

            string mode = dryRun ? "Simulated" : (sendToRecycleBin ? "Moved to Recycle Bin" : "Deleted");
            string status = $"{mode}: {report.FilesReclaimed:N0} files ({Fmt.Bytes(report.BytesReclaimed)})";
            if (report.FilesFailed > 0)
            {
                status += $" • {report.FilesFailed:N0} locked/protected files skipped";
            }
            if (report.ItemsFailed > 0)
            {
                status += $" • {report.ItemsFailed} item(s) failed ({report.FirstError})";
            }
            CleanupStatusText.Text = status;

            if (!dryRun)
            {
                _session.NotifyReclaimableChanged();
                AnalyzeCleanup(force: true);
                // AnalyzeCleanup overwrites the status line; restore the result summary afterwards.
                CleanupStatusText.Text = status;
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
