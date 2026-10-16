using System;
using System.Collections.Generic;
using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Brushes;
using Microsoft.Graphics.Canvas.Text;
using Microsoft.Graphics.Canvas.UI;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using StorageIntelligence.Native;
using Windows.Foundation;
using Windows.UI;

namespace StorageIntelligence.Controls;

public readonly record struct BreadcrumbItem(ulong NodeId, string Name);

/// <summary>
/// Hardware-accelerated squarified treemap control powered by Win2D (ADR-010).
/// </summary>
public sealed partial class TreemapControl : UserControl
{
    private StorageTreeService? _tree;
    private TreemapRect[] _rects = Array.Empty<TreemapRect>();
    private readonly List<BreadcrumbItem> _breadcrumbs = new();

    private ulong _currentRootId;
    private ulong? _selectedNodeId;
    private ulong? _hoveredNodeId;

    private CanvasTextFormat? _labelFormat;
    private CanvasTextFormat? _folderLabelFormat;
    private CanvasTextFormat? _subLabelFormat;
    private CanvasLinearGradientBrush? _tileSheen;

    // Per-layout caches so Canvas_Draw never calls into the native tree (computed once per RecomputeLayout).
    private string?[] _labels = Array.Empty<string?>();
    private string?[] _sizeLabels = Array.Empty<string?>();
    private Color[] _fills = Array.Empty<Color>();
    private readonly Dictionary<ulong, int> _indexByNode = new();

    private DispatcherQueueTimer? _resizeTimer;
    private static readonly TimeSpan ResizeDebounce = TimeSpan.FromMilliseconds(50);
    private const float TileRadius = 6f;

    private readonly Dictionary<FileCategory, Color> _categoryColors = new();

    public event EventHandler<NodeInfo>? NodeSelected;
    public event EventHandler<NodeInfo?>? NodeHovered;
    public event EventHandler<IReadOnlyList<BreadcrumbItem>>? DrillDownChanged;

    public StorageTreeService? Tree => _tree;
    public ulong CurrentRootId => _currentRootId;
    public ulong? SelectedNodeId => _selectedNodeId;
    public IReadOnlyList<BreadcrumbItem> Breadcrumbs => _breadcrumbs;

    public TreemapControl()
    {
        InitializeComponent();
        InitializeCategoryPalette();
    }

    private void InitializeCategoryPalette()
    {
        foreach (FileCategory cat in Enum.GetValues<FileCategory>())
        {
            _categoryColors[cat] = StorageIntelligence.Styles.Palette.CategoryColor(cat);
        }
    }

    /// <summary>
    /// Loads a new storage tree and sets the treemap view to its root.
    /// </summary>
    public void SetTree(StorageTreeService? tree)
    {
        _tree = tree;
        _breadcrumbs.Clear();
        _selectedNodeId = null;
        _hoveredNodeId = null;

        if (_tree != null)
        {
            _currentRootId = _tree.GetRootId();
            string rootName = _tree.GetNodeName(_currentRootId);
            if (string.IsNullOrEmpty(rootName)) rootName = "Root";
            _breadcrumbs.Add(new BreadcrumbItem(_currentRootId, rootName));
            EmptyStatePanel.Visibility = Visibility.Collapsed;
        }
        else
        {
            _currentRootId = 0;
            EmptyStatePanel.Visibility = Visibility.Visible;
        }

        DrillDownChanged?.Invoke(this, _breadcrumbs);
        RecomputeLayout();
    }

    /// <summary>
    /// Drills down into a directory node as the new viewport root.
    /// </summary>
    public void DrillDown(ulong nodeId)
    {
        if (_tree == null) return;
        var info = _tree.GetNodeInfo(nodeId);
        if (info.Kind != NodeKind.Directory) return;

        _currentRootId = nodeId;
        _breadcrumbs.Add(new BreadcrumbItem(nodeId, info.Name));
        _selectedNodeId = nodeId;

        DrillDownChanged?.Invoke(this, _breadcrumbs);
        NodeSelected?.Invoke(this, info);
        RecomputeLayout();
    }

    /// <summary>
    /// Navigates to a specific ancestor node already in the breadcrumb trail.
    /// </summary>
    public void NavigateTo(ulong nodeId)
    {
        if (_tree == null) return;

        int index = _breadcrumbs.FindIndex(b => b.NodeId == nodeId);
        if (index >= 0)
        {
            _breadcrumbs.RemoveRange(index + 1, _breadcrumbs.Count - (index + 1));
            _currentRootId = nodeId;
            _selectedNodeId = nodeId;

            var info = _tree.GetNodeInfo(nodeId);
            DrillDownChanged?.Invoke(this, _breadcrumbs);
            NodeSelected?.Invoke(this, info);
            RecomputeLayout();
        }
    }

    /// <summary>
    /// Navigates up one level to the parent directory.
    /// </summary>
    public void NavigateUp()
    {
        if (_breadcrumbs.Count > 1)
        {
            NavigateTo(_breadcrumbs[^2].NodeId);
        }
    }

    /// <summary>
    /// Navigates the treemap to the parent container of the specified node (or the directory itself),
    /// reconstructs the breadcrumbs from root, and highlights the target node.
    /// </summary>
    public void NavigateAndSelect(ulong nodeId)
    {
        if (_tree == null) return;

        var info = _tree.GetNodeInfo(nodeId, includeFullPath: true);
        ulong viewRootId = info.Kind == NodeKind.Directory ? nodeId : (info.ParentId ?? _tree.GetRootId());

        var ancestors = _tree.GetAncestors(viewRootId);
        _breadcrumbs.Clear();
        for (int i = ancestors.Length - 1; i >= 0; i--)
        {
            ulong aId = ancestors[i];
            string aName = _tree.GetNodeName(aId);
            if (string.IsNullOrEmpty(aName)) aName = "Root";
            _breadcrumbs.Add(new BreadcrumbItem(aId, aName));
        }

        _currentRootId = viewRootId;
        _selectedNodeId = nodeId;

        DrillDownChanged?.Invoke(this, _breadcrumbs);
        NodeSelected?.Invoke(this, info);
        RecomputeLayout();
    }

    public void RecomputeLayout()
    {
        if (_tree == null || Canvas.ActualWidth <= 0 || Canvas.ActualHeight <= 0)
        {
            _rects = Array.Empty<TreemapRect>();
            BuildCaches();
            Canvas.Invalidate();
            return;
        }

        try
        {
            _rects = _tree.ComputeLayout(
                _currentRootId,
                (float)Canvas.ActualWidth,
                (float)Canvas.ActualHeight,
                maxDepth: 2,
                minSizePx: 3.0f);
        }
        catch (Exception)
        {
            _rects = Array.Empty<TreemapRect>();
        }

        BuildCaches();
        Canvas.Invalidate();
    }

    /// <summary>
    /// Resolves names, size strings, fill colors and the node-to-rect index once per layout so that
    /// <see cref="Canvas_Draw"/> never calls into the native tree.
    /// </summary>
    private void BuildCaches()
    {
        int n = _rects.Length;
        _labels = new string?[n];
        _sizeLabels = new string?[n];
        _fills = new Color[n];
        _indexByNode.Clear();

        for (int i = 0; i < n; i++)
        {
            ref readonly var r = ref _rects[i];
            _indexByNode.TryAdd(r.NodeId, i);

            Color fill = _categoryColors.TryGetValue(r.Category, out var c) ? c : Colors.DimGray;
            if (r.Depth > 1)
            {
                // Slightly dim deeper nesting levels to create depth perception
                fill = Color.FromArgb(fill.A, (byte)(fill.R * 0.86), (byte)(fill.G * 0.86), (byte)(fill.B * 0.86));
            }
            _fills[i] = fill;

            if (_tree != null && r.Width >= 48 && r.Height >= 20)
            {
                try
                {
                    if (r.Height >= 36)
                    {
                        var info = _tree.GetNodeInfo(r.NodeId);
                        _labels[i] = info.Name;
                        _sizeLabels[i] = Services.Fmt.Bytes(info.Size);
                    }
                    else
                    {
                        _labels[i] = _tree.GetNodeName(r.NodeId);
                    }
                }
                catch
                {
                    // Leave the tile unlabeled if the lookup fails
                }
            }
        }
    }

    private void Canvas_CreateResources(CanvasControl sender, CanvasCreateResourcesEventArgs args)
    {
        _labelFormat = new CanvasTextFormat
        {
            FontSize = 11.5f,
            FontWeight = Microsoft.UI.Text.FontWeights.Normal,
            HorizontalAlignment = CanvasHorizontalAlignment.Left,
            VerticalAlignment = CanvasVerticalAlignment.Top,
            WordWrapping = CanvasWordWrapping.NoWrap,
            TrimmingGranularity = CanvasTextTrimmingGranularity.Character,
        };

        _folderLabelFormat = new CanvasTextFormat
        {
            FontSize = 11.5f,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            HorizontalAlignment = CanvasHorizontalAlignment.Left,
            VerticalAlignment = CanvasVerticalAlignment.Top,
            WordWrapping = CanvasWordWrapping.NoWrap,
            TrimmingGranularity = CanvasTextTrimmingGranularity.Character,
        };

        _subLabelFormat = new CanvasTextFormat
        {
            FontSize = 10f,
            FontFamily = "Cascadia Mono, Consolas",
            HorizontalAlignment = CanvasHorizontalAlignment.Left,
            VerticalAlignment = CanvasVerticalAlignment.Top,
            WordWrapping = CanvasWordWrapping.NoWrap,
            TrimmingGranularity = CanvasTextTrimmingGranularity.Character,
        };

        // Unit-square gradient; its Transform is set per tile (one brush, no per-tile allocation).
        _tileSheen = new CanvasLinearGradientBrush(sender, new[]
        {
            new CanvasGradientStop { Position = 0f, Color = Color.FromArgb(46, 255, 255, 255) },
            new CanvasGradientStop { Position = 1f, Color = Color.FromArgb(56, 0, 0, 0) },
        })
        {
            StartPoint = new Vector2(0, 0),
            EndPoint = new Vector2(1, 1),
        };
    }

    private void Canvas_Draw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var ds = args.DrawingSession;
        if (_tree == null || _rects.Length == 0) return;

        Color accent = StorageIntelligence.Styles.ThemeBrushes.GetColor("SpaceAccentColor");
        Color edge = Color.FromArgb(38, 255, 255, 255);
        Color folderEdge = Color.FromArgb(80, 255, 255, 255);
        Color shadow = Color.FromArgb(150, 0, 0, 0);
        Color subText = Color.FromArgb(215, 255, 255, 255);

        for (int i = 0; i < _rects.Length; i++)
        {
            ref readonly var r = ref _rects[i];
            if (r.Width < 2 || r.Height < 2) continue;

            // 1px gutter between tiles keeps neighbors visually separate
            float x = r.X + 1, y = r.Y + 1, w = Math.Max(1, r.Width - 2), h = Math.Max(1, r.Height - 2);
            float radius = Math.Min(TileRadius, Math.Min(w, h) / 4f);
            bool isDir = r.Kind == NodeKind.Directory;

            ds.FillRoundedRectangle(x, y, w, h, radius, radius, _fills[i]);

            if (_tileSheen != null)
            {
                _tileSheen.Transform = Matrix3x2.CreateScale(w, h) * Matrix3x2.CreateTranslation(x, y);
                ds.FillRoundedRectangle(x, y, w, h, radius, radius, _tileSheen);
            }

            ds.DrawRoundedRectangle(x, y, w, h, radius, radius, isDir ? folderEdge : edge, 1.0f);

            string? label = _labels[i];
            if (label != null)
            {
                var fmt = isDir ? _folderLabelFormat : _labelFormat;
                float tw = w - 14;
                if (fmt != null && tw > 8)
                {
                    ds.DrawText(label, new Rect(x + 8, y + 6, tw, 16), shadow, fmt);
                    ds.DrawText(label, new Rect(x + 7, y + 5, tw, 16), Colors.White, fmt);

                    string? sizeText = _sizeLabels[i];
                    if (sizeText != null && _subLabelFormat != null)
                    {
                        ds.DrawText(sizeText, new Rect(x + 8, y + 22, tw, 14), shadow, _subLabelFormat);
                        ds.DrawText(sizeText, new Rect(x + 7, y + 21, tw, 14), subText, _subLabelFormat);
                    }
                }
            }
        }

        // Hover: soft layered glow plus a crisp white outline
        if (_hoveredNodeId.HasValue && _indexByNode.TryGetValue(_hoveredNodeId.Value, out int hi))
        {
            ref readonly var r = ref _rects[hi];
            float x = r.X + 1, y = r.Y + 1, w = Math.Max(1, r.Width - 2), h = Math.Max(1, r.Height - 2);
            float radius = Math.Min(TileRadius, Math.Min(w, h) / 4f);
            for (int k = 4; k >= 1; k--)
            {
                byte a = (byte)(26 * (5 - k));
                ds.DrawRoundedRectangle(x - k, y - k, w + 2 * k, h + 2 * k, radius + k, radius + k,
                    Color.FromArgb(a, accent.R, accent.G, accent.B), 2.0f);
            }
            ds.DrawRoundedRectangle(x, y, w, h, radius, radius, Colors.White, 1.5f);
        }

        // Selection ring in the accent color
        if (_selectedNodeId.HasValue && _indexByNode.TryGetValue(_selectedNodeId.Value, out int si))
        {
            ref readonly var r = ref _rects[si];
            float x = r.X + 1, y = r.Y + 1, w = Math.Max(1, r.Width - 2), h = Math.Max(1, r.Height - 2);
            float radius = Math.Min(TileRadius, Math.Min(w, h) / 4f);
            ds.DrawRoundedRectangle(x, y, w, h, radius, radius, accent, 2.5f);
        }
    }

    private void Canvas_PointerMoved(object sender, PointerRoutedEventArgs e)
    {
        if (_tree == null || _rects.Length == 0) return;

        var point = e.GetCurrentPoint(Canvas).Position;
        var hit = TreemapService.FindNodeAtPoint((float)point.X, (float)point.Y, _rects);

        if (hit.HasValue)
        {
            if (_hoveredNodeId != hit.Value.NodeId)
            {
                _hoveredNodeId = hit.Value.NodeId;
                Canvas.Invalidate();

                try
                {
                    var info = _tree.GetNodeInfo(hit.Value.NodeId, includeFullPath: true);
                    NodeHovered?.Invoke(this, info);

                    // Update tooltip
                    TooltipTitle.Text = info.Name;
                    TooltipSize.Text = $"{Services.Fmt.Bytes(info.Size)}  •  {info.Kind}";
                    TooltipPath.Text = info.FullPath ?? string.Empty;

                    HoverPopup.HorizontalOffset = point.X + 16;
                    HoverPopup.VerticalOffset = point.Y + 16;
                    HoverPopup.IsOpen = true;
                }
                catch
                {
                    HoverPopup.IsOpen = false;
                }
            }
            else
            {
                // Follow cursor
                HoverPopup.HorizontalOffset = point.X + 16;
                HoverPopup.VerticalOffset = point.Y + 16;
            }
        }
        else
        {
            if (_hoveredNodeId.HasValue)
            {
                _hoveredNodeId = null;
                HoverPopup.IsOpen = false;
                NodeHovered?.Invoke(this, null);
                Canvas.Invalidate();
            }
        }
    }

    private void Canvas_PointerExited(object sender, PointerRoutedEventArgs e)
    {
        if (_hoveredNodeId.HasValue)
        {
            _hoveredNodeId = null;
            HoverPopup.IsOpen = false;
            NodeHovered?.Invoke(this, null);
            Canvas.Invalidate();
        }
    }

    private void Canvas_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (_tree == null || _rects.Length == 0) return;

        var point = e.GetCurrentPoint(Canvas).Position;
        var hit = TreemapService.FindNodeAtPoint((float)point.X, (float)point.Y, _rects);

        if (hit.HasValue)
        {
            _selectedNodeId = hit.Value.NodeId;
            Canvas.Invalidate();

            try
            {
                var info = _tree.GetNodeInfo(hit.Value.NodeId, includeFullPath: true);
                NodeSelected?.Invoke(this, info);
            }
            catch { }
        }
    }

    private void Canvas_DoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if (_tree == null || _rects.Length == 0) return;

        var point = e.GetPosition(Canvas);
        var hit = TreemapService.FindNodeAtPoint((float)point.X, (float)point.Y, _rects);

        if (hit.HasValue && hit.Value.Kind == NodeKind.Directory)
        {
            DrillDown(hit.Value.NodeId);
        }
    }

    /// <summary>Debounced: a window drag raises many SizeChanged events; recompute once ~50ms after the last.</summary>
    private void Canvas_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_resizeTimer == null)
        {
            _resizeTimer = DispatcherQueue.CreateTimer();
            _resizeTimer.Interval = ResizeDebounce;
            _resizeTimer.IsRepeating = false;
            _resizeTimer.Tick += (_, _) => RecomputeLayout();
        }

        _resizeTimer.Stop();
        _resizeTimer.Start();
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
}
