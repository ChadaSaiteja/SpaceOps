using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using StorageIntelligence.Controls;
using StorageIntelligence.Native;
using StorageIntelligence.Services;
using StorageIntelligence.Styles;

namespace StorageIntelligence.Pages;

/// <summary>Treemap destination: breadcrumbs, search, depth, the Direct2D treemap and the selection details panel.</summary>
public sealed partial class TreemapPage : Page
{
    private readonly AppSession _session = AppSession.Current;
    private CancellationTokenSource? _searchCts;
    private NodeInfo? _selectedNode;
    private IReadOnlyList<BreadcrumbItem> _currentBreadcrumbs = Array.Empty<BreadcrumbItem>();

    public TreemapPage()
    {
        InitializeComponent();
        NavigationCacheMode = NavigationCacheMode.Required;
        LegendRepeater.ItemsSource = Palette.Legend();

        // Stay subscribed for the app lifetime so a cached (hidden) page never holds a disposed tree.
        _session.TreeChanged += OnTreeChanged;
        _session.TreeUpdated += OnTreeUpdated;

        if (_session.Tree != null)
        {
            Treemap.SetTree(_session.Tree);
        }
    }

    private void OnTreeChanged()
    {
        _selectedNode = null;
        ResetDetails();
        Treemap.SetTree(_session.Tree);
    }

    private void OnTreeUpdated()
    {
        Treemap.RecomputeLayout();
    }

    private void ResetDetails()
    {
        DetailTitle.Text = "No Selection";
        DetailPath.Text = "—";
        DetailSize.Text = "—";
        DetailCategory.Text = "—";
        DirStatsGrid.Visibility = Visibility.Collapsed;
        DrillDownButton.IsEnabled = false;
        OpenExplorerButton.IsEnabled = false;
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
        DetailSize.Text = Fmt.Bytes(info.Size);

        if (info.Kind == NodeKind.Directory)
        {
            DetailIcon.Glyph = ""; // Folder icon
            DetailCategory.Text = "Folder";
            DirStatsGrid.Visibility = Visibility.Visible;
            DetailFileCount.Text = $"{info.FileCount:N0}";
            DetailDirCount.Text = $"{info.DirCount:N0}";
            DrillDownButton.IsEnabled = true;
        }
        else
        {
            DetailIcon.Glyph = ""; // File icon
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

        try
        {
            Fmt.RevealInExplorer(_selectedNode.Value.FullPath);
        }
        catch (Exception ex)
        {
            _session.SetStatus($"Failed to open explorer: {ex.Message}");
        }
    }

    private async void SearchBox_TextChanged(AutoSuggestBox sender, AutoSuggestBoxTextChangedEventArgs args)
    {
        if (args.Reason != AutoSuggestionBoxTextChangeReason.UserInput) return;

        var tree = _session.Tree;
        string query = sender.Text.Trim();
        if (tree == null || string.IsNullOrWhiteSpace(query))
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
            var results = await tree.SearchAsync(query, maxResults: 30);
            sw.Stop();

            if (token.IsCancellationRequested) return;

            sender.ItemsSource = results.Select(r => new SearchResultViewModel(r)).ToList();

            if (results.Length > 0)
            {
                _session.SetStatus($"Found {results.Length} matching items in {sw.ElapsedMilliseconds} ms for '{query}'");
            }
        }
        catch (OperationCanceledException)
        {
            // Ignore debounced cancellations
        }
        catch (Exception ex)
        {
            _session.SetStatus($"Search error: {ex.Message}");
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

        var tree = _session.Tree;
        string query = args.QueryText?.Trim() ?? string.Empty;
        if (tree == null || string.IsNullOrWhiteSpace(query)) return;

        try
        {
            var results = await tree.SearchAsync(query, maxResults: 10);
            if (results.Length > 0)
            {
                Treemap.NavigateAndSelect(results[0].NodeId);
            }
            else
            {
                _session.SetStatus($"No files or folders found matching '{query}'");
            }
        }
        catch (Exception ex)
        {
            _session.SetStatus($"Search failed: {ex.Message}");
        }
    }

    private void DepthComboBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        // Reserved for dynamic depth adjustment in future revisions
    }
}
