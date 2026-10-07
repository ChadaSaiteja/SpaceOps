using System;
using System.Collections.Generic;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Text;
using Microsoft.Graphics.Canvas.UI;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
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
    private CanvasTextFormat? _subLabelFormat;

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
            string hex = TreemapService.GetCategoryColorHex(cat);
            _categoryColors[cat] = HexToColor(hex);
        }
    }

    private static Color HexToColor(string hex)
    {
        if (hex.StartsWith("#")) hex = hex[1..];
        byte r = Convert.ToByte(hex[0..2], 16);
        byte g = Convert.ToByte(hex[2..4], 16);
        byte b = Convert.ToByte(hex[4..6], 16);
        return Color.FromArgb(255, r, g, b);
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

        Canvas.Invalidate();
    }

    private void Canvas_CreateResources(CanvasControl sender, CanvasCreateResourcesEventArgs args)
    {
        _labelFormat = new CanvasTextFormat
        {
            FontSize = 11,
            HorizontalAlignment = CanvasHorizontalAlignment.Left,
            VerticalAlignment = CanvasVerticalAlignment.Top,
            WordWrapping = CanvasWordWrapping.NoWrap,
            TrimmingGranularity = CanvasTextTrimmingGranularity.Character
        };

        _subLabelFormat = new CanvasTextFormat
        {
            FontSize = 9.5f,
            HorizontalAlignment = CanvasHorizontalAlignment.Left,
            VerticalAlignment = CanvasVerticalAlignment.Top,
            WordWrapping = CanvasWordWrapping.NoWrap,
            TrimmingGranularity = CanvasTextTrimmingGranularity.Character
        };
    }

    private void Canvas_Draw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var ds = args.DrawingSession;
        if (_tree == null || _rects.Length == 0) return;

        Color borderColor = Color.FromArgb(80, 20, 20, 20);

        // 1. Draw all rectangle tiles
        for (int i = 0; i < _rects.Length; i++)
        {
            ref readonly var r = ref _rects[i];

            Color fillColor = _categoryColors.TryGetValue(r.Category, out var c) ? c : Colors.DimGray;

            // Slightly dim deeper nesting levels to create depth perception
            if (r.Depth > 1)
            {
                fillColor = Color.FromArgb(
                    fillColor.A,
                    (byte)(fillColor.R * 0.88),
                    (byte)(fillColor.G * 0.88),
                    (byte)(fillColor.B * 0.88));
            }

            // Fill tile
            ds.FillRectangle(r.X, r.Y, r.Width, r.Height, fillColor);

            // Draw border
            ds.DrawRectangle(r.X, r.Y, r.Width, r.Height, borderColor, 1.0f);

            // Draw text labels if space permits
            if (r.Width >= 48 && r.Height >= 20 && _labelFormat != null)
            {
                try
                {
                    string name = _tree.GetNodeName(r.NodeId);
                    Rect textRect = new Rect(r.X + 3, r.Y + 2, r.Width - 6, r.Height - 4);

                    // Drop shadow for legibility over colored tiles
                    ds.DrawText(name, (float)textRect.X + 1, (float)textRect.Y + 1, Colors.Black, _labelFormat);
                    ds.DrawText(name, (float)textRect.X, (float)textRect.Y, Colors.White, _labelFormat);

                    // Sub-label for size if height >= 36px
                    if (r.Height >= 36 && _subLabelFormat != null)
                    {
                        var info = _tree.GetNodeInfo(r.NodeId);
                        string sizeStr = FormatBytes(info.Size);
                        ds.DrawText(sizeStr, (float)textRect.X + 1, (float)textRect.Y + 14, Colors.Black, _subLabelFormat);
                        ds.DrawText(sizeStr, (float)textRect.X, (float)textRect.Y + 13, Color.FromArgb(220, 240, 240, 240), _subLabelFormat);
                    }
                }
                catch
                {
                    // Ignore node lookup errors during active drawing
                }
            }
        }

        // 2. Draw hover highlight outline
        if (_hoveredNodeId.HasValue)
        {
            for (int i = 0; i < _rects.Length; i++)
            {
                ref readonly var r = ref _rects[i];
                if (r.NodeId == _hoveredNodeId.Value)
                {
                    ds.DrawRectangle(r.X, r.Y, r.Width, r.Height, Colors.White, 2.0f);
                    break;
                }
            }
        }

        // 3. Draw selection highlight outline
        if (_selectedNodeId.HasValue)
        {
            for (int i = 0; i < _rects.Length; i++)
            {
                ref readonly var r = ref _rects[i];
                if (r.NodeId == _selectedNodeId.Value)
                {
                    ds.DrawRectangle(r.X, r.Y, r.Width, r.Height, Color.FromArgb(255, 96, 205, 255), 2.5f);
                    break;
                }
            }
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
                    TooltipSize.Text = $"{FormatBytes(info.Size)}  •  {info.Kind}";
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

    private void Canvas_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        RecomputeLayout();
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
