using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Microsoft.UI.Xaml.Shapes;
using StorageIntelligence.Native;
using StorageIntelligence.Services;
using StorageIntelligence.Styles;
using Windows.Foundation;

namespace StorageIntelligence.Pages;

/// <summary>
/// Overview destination: usage donut by category, top consumers, reclaimable-now total and quick actions.
/// All numbers come from the in-memory tree plus the existing Cleanup/DevStorage services.
/// </summary>
public sealed partial class OverviewPage : Page
{
    private const double DonutSize = 170;
    private const double DonutThickness = 24;

    private readonly AppSession _session = AppSession.Current;
    private bool _isCurrent;
    private bool _dirty = true;
    private int _generation;
    private int _reclaimGeneration;

    public OverviewPage()
    {
        InitializeComponent();
        NavigationCacheMode = NavigationCacheMode.Required;

        _session.TreeChanged += () => { _dirty = true; if (_isCurrent) _ = RefreshAsync(); };
        _session.TreeUpdated += () => { _dirty = true; if (_isCurrent) _ = RefreshAsync(); };
        _session.ReclaimableInvalidated += () => { if (_isCurrent && _session.Tree != null) _ = RefreshReclaimableAsync(); };
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        base.OnNavigatedTo(e);
        _isCurrent = true;
        if (_dirty)
        {
            _ = RefreshAsync();
        }
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        base.OnNavigatedFrom(e);
        _isCurrent = false;
    }

    private async Task RefreshAsync()
    {
        var tree = _session.Tree;
        int gen = ++_generation;
        _dirty = false;

        if (tree == null)
        {
            EmptyStatePanel.Visibility = Visibility.Visible;
            ContentPanel.Visibility = Visibility.Collapsed;
            SubtitleText.Text = "Where your storage goes, and what you can safely reclaim.";
            return;
        }

        EmptyStatePanel.Visibility = Visibility.Collapsed;
        ContentPanel.Visibility = Visibility.Visible;

        try
        {
            var snapshot = await OverviewService.ComputeAsync(tree);
            if (gen != _generation || !ReferenceEquals(tree, _session.Tree)) return;
            Apply(snapshot);
        }
        catch (Exception ex)
        {
            _session.SetStatus($"Overview error: {ex.Message}");
        }

        await RefreshReclaimableAsync();
    }

    private async Task RefreshReclaimableAsync()
    {
        var tree = _session.Tree;
        if (tree == null) return;

        int gen = ++_reclaimGeneration;
        ReclaimRing.IsActive = true;
        ReclaimTotalText.Text = "…";

        try
        {
            var summary = await OverviewService.ComputeReclaimableAsync(tree);
            if (gen != _reclaimGeneration || !ReferenceEquals(tree, _session.Tree)) return;

            ReclaimTotalText.Text = Fmt.Bytes(summary.TotalBytes);
            ReclaimJunkLabel.Text = $"System junk ({summary.SystemJunkCategories} categories)";
            ReclaimJunkValue.Text = Fmt.Bytes(summary.SystemJunkBytes);
            ReclaimDevLabel.Text = $"Dormant developer artifacts ({summary.DevArtifacts})";
            ReclaimDevValue.Text = Fmt.Bytes(summary.DevBytes);
        }
        catch (Exception ex)
        {
            ReclaimTotalText.Text = "—";
            _session.SetStatus($"Reclaimable analysis failed: {ex.Message}");
        }
        finally
        {
            if (gen == _reclaimGeneration)
            {
                ReclaimRing.IsActive = false;
            }
        }
    }

    private void Apply(OverviewSnapshot snap)
    {
        SubtitleText.Text = $"{snap.RootName}  •  {snap.FileCount:N0} files in {snap.DirCount:N0} folders";
        DonutTotalText.Text = Fmt.Bytes(snap.TotalBytes);

        var rows = new List<SliceRowViewModel>();
        var sliceBrushes = new List<(double Fraction, Brush Brush)>();
        foreach (var s in snap.Slices)
        {
            Brush brush = s.Category.HasValue ? Palette.CategoryBrush(s.Category.Value) : ThemeBrushes.Muted;
            rows.Add(new SliceRowViewModel(s.Name, brush, Fmt.Bytes(s.Bytes), $"{s.Fraction * 100:0.#}%"));
            sliceBrushes.Add((s.Fraction, brush));
        }
        SliceList.ItemsSource = rows;
        DrawDonut(sliceBrushes);
        SliceFootnote.Text = $"Categories are classified from the {snap.SlicedFileCount:N0} largest files; every other byte is counted as \"Smaller files\".";

        var consumers = snap.TopConsumers.Select(c => new ConsumerRowViewModel(
            c.NodeId,
            c.Name,
            c.Kind == NodeKind.Directory ? "" : "",
            Fmt.Bytes(c.Bytes),
            $"{c.Fraction * 100:0.#}%",
            c.Fraction * 100,
            c.Kind == NodeKind.Directory ? ThemeBrushes.Accent : ThemeBrushes.Accent2)).ToList();
        ConsumerList.ItemsSource = consumers;
        ConsumerEmptyText.Visibility = consumers.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private void DrawDonut(IReadOnlyList<(double Fraction, Brush Brush)> slices)
    {
        DonutCanvas.Children.Clear();

        double center = DonutSize / 2;
        double radius = (DonutSize - DonutThickness) / 2;

        // Track ring
        var track = new Ellipse
        {
            Width = radius * 2,
            Height = radius * 2,
            Stroke = ThemeBrushes.Border,
            StrokeThickness = DonutThickness,
        };
        Canvas.SetLeft(track, center - radius);
        Canvas.SetTop(track, center - radius);
        DonutCanvas.Children.Add(track);

        double startAngle = -90;
        foreach (var (fraction, brush) in slices)
        {
            if (fraction <= 0) continue;

            double sweep = Math.Min(fraction * 360.0, 359.9);
            if (sweep < 0.5) { startAngle += sweep; continue; }

            Point start = PointOnCircle(center, radius, startAngle);
            Point end = PointOnCircle(center, radius, startAngle + sweep);

            var figure = new PathFigure { StartPoint = start, IsClosed = false };
            figure.Segments.Add(new ArcSegment
            {
                Point = end,
                Size = new Size(radius, radius),
                SweepDirection = SweepDirection.Clockwise,
                IsLargeArc = sweep > 180,
            });

            var geometry = new PathGeometry();
            geometry.Figures.Add(figure);
            DonutCanvas.Children.Add(new Microsoft.UI.Xaml.Shapes.Path
            {
                Data = geometry,
                Stroke = brush,
                StrokeThickness = DonutThickness,
            });

            startAngle += sweep;
        }
    }

    private static Point PointOnCircle(double center, double radius, double angleDegrees)
    {
        double rad = angleDegrees * Math.PI / 180.0;
        return new Point(center + radius * Math.Cos(rad), center + radius * Math.Sin(rad));
    }

    // ----- Quick actions -----

    private async void QuickScan_Click(object sender, RoutedEventArgs e) => await _session.ScanAsync();

    private async void QuickSync_Click(object sender, RoutedEventArgs e) => await _session.SyncAsync();

    private void QuickTreemap_Click(object sender, RoutedEventArgs e) => _session.RequestNavigation("treemap");

    private void ReviewCleanup_Click(object sender, RoutedEventArgs e) => _session.RequestNavigation("cleanup");

    private void QuickDeveloper_Click(object sender, RoutedEventArgs e) => _session.RequestNavigation("developer");

    private void QuickApplications_Click(object sender, RoutedEventArgs e) => _session.RequestNavigation("applications");
}
