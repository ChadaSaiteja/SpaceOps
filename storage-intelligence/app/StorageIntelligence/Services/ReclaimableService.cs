using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using Microsoft.UI.Xaml.Media;
using StorageIntelligence.Native;
using StorageIntelligence.Styles;

namespace StorageIntelligence.Services;

public enum CandidateSource
{
    SystemJunk,
    DevArtifact,
    AppLeftover,
}

/// <summary>
/// One row in the unified cleanup list, regardless of where it was discovered
/// (system junk rules, dormant developer artifacts, orphaned application leftovers).
/// </summary>
public sealed class UnifiedCandidate : INotifyPropertyChanged
{
    private bool _isSelected;

    public CandidateSource Source { get; init; }
    public string Name { get; init; } = string.Empty;
    public string Description { get; init; } = string.Empty;
    public string PathDisplay { get; init; } = string.Empty;
    public ulong Bytes { get; init; }
    public ulong FileCount { get; init; }
    public RiskLevel Risk { get; init; }
    public bool IsProtected { get; init; }

    /// <summary>Rule id for <see cref="CandidateSource.SystemJunk"/> candidates.</summary>
    public CleanupRuleId? RuleId { get; init; }

    /// <summary>Folder path for dev artifacts and app leftovers.</summary>
    public string? Path { get; init; }

    public bool CanSelect => !IsProtected;

    public bool IsSelected
    {
        get => _isSelected;
        set
        {
            if (_isSelected != value)
            {
                _isSelected = value;
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(IsSelected)));
            }
        }
    }

    public string FormattedSize => Fmt.Bytes(Bytes);
    public string FileCountText => FileCount > 0 ? $"{FileCount:N0} files" : "Single entity";

    public string SourceText => Source switch
    {
        CandidateSource.SystemJunk => "System junk",
        CandidateSource.DevArtifact => "Dev artifact",
        _ => "App leftover",
    };

    public SolidColorBrush SourceBrush => Source switch
    {
        CandidateSource.SystemJunk => ThemeBrushes.Accent,
        CandidateSource.DevArtifact => ThemeBrushes.Green,
        _ => ThemeBrushes.Orange,
    };

    public string RiskText => IsProtected ? "Protected" : Risk switch
    {
        RiskLevel.Low => "Low Risk",
        RiskLevel.Medium => "Medium Risk",
        _ => "Review Needed",
    };

    public SolidColorBrush RiskBrush => IsProtected ? ThemeBrushes.Red : Risk switch
    {
        RiskLevel.Low => ThemeBrushes.Green,
        RiskLevel.Medium => ThemeBrushes.Amber,
        _ => ThemeBrushes.Red,
    };

    public event PropertyChangedEventHandler? PropertyChanged;
}

/// <summary>The single reclaimable list and totals shared by Cleanup and Overview.</summary>
public sealed class ReclaimableSet
{
    public static ReclaimableSet Empty { get; } = new(new List<UnifiedCandidate>(), new List<string>());

    public ReclaimableSet(List<UnifiedCandidate> candidates, List<string> errors)
    {
        Candidates = candidates;
        Errors = errors;
    }

    public List<UnifiedCandidate> Candidates { get; }
    public List<string> Errors { get; }

    /// <summary>Protected candidates are listed but never counted as reclaimable.</summary>
    public IEnumerable<UnifiedCandidate> Reclaimable => Candidates.Where(c => !c.IsProtected);

    public ulong TotalBytes => Reclaimable.Aggregate(0UL, (sum, c) => sum + c.Bytes);

    public ulong BytesFor(CandidateSource source) =>
        Reclaimable.Where(c => c.Source == source).Aggregate(0UL, (sum, c) => sum + c.Bytes);

    public int CountFor(CandidateSource source) => Reclaimable.Count(c => c.Source == source);
}

public sealed record CleanupRunReport(
    ulong FilesReclaimed,
    ulong BytesReclaimed,
    ulong FilesFailed,
    int ItemsFailed,
    string? FirstError);

/// <summary>
/// Builds and executes the unified cleanup list. Safety is unchanged: system rules and dev artifacts go
/// through the Rust core (blacklist + Recycle Bin routing), app leftovers always use the Recycle Bin and are
/// additionally checked against the protected-path blacklist.
/// </summary>
public static class ReclaimableService
{
    public static async Task<ReclaimableSet> LoadAsync(StorageTreeService tree)
    {
        var candidates = new List<UnifiedCandidate>();
        var errors = new List<string>();

        var junkTask = LoadJunkAsync(tree);
        var devTask = LoadDevAsync(tree);
        var leftoverTask = LoadLeftoversAsync(tree);

        await Task.WhenAll(junkTask, devTask, leftoverTask);

        foreach (var (items, error) in new[] { junkTask.Result, devTask.Result, leftoverTask.Result })
        {
            candidates.AddRange(items);
            if (error != null) errors.Add(error);
        }

        candidates.Sort((a, b) =>
        {
            int byProtected = a.IsProtected.CompareTo(b.IsProtected);
            return byProtected != 0 ? byProtected : b.Bytes.CompareTo(a.Bytes);
        });

        return new ReclaimableSet(candidates, errors);
    }

    private static async Task<(List<UnifiedCandidate>, string?)> LoadJunkAsync(StorageTreeService tree)
    {
        var list = new List<UnifiedCandidate>();
        try
        {
            foreach (var c in await tree.DetectCleanupCandidatesAsync())
            {
                list.Add(new UnifiedCandidate
                {
                    Source = CandidateSource.SystemJunk,
                    Name = c.Name,
                    Description = c.Description,
                    PathDisplay = c.PathDisplay,
                    Bytes = c.TotalBytes,
                    FileCount = c.FileCount,
                    Risk = c.RiskLevel,
                    IsProtected = c.IsProtected,
                    RuleId = c.RuleId,
                    // Low-risk items are selected by default; medium/high require conscious opt-in
                    IsSelected = c.RiskLevel == RiskLevel.Low && c.TotalBytes > 0 && !c.IsProtected,
                });
            }
            return (list, null);
        }
        catch (Exception ex)
        {
            return (list, $"System junk analysis failed: {ex.Message}");
        }
    }

    private static async Task<(List<UnifiedCandidate>, string?)> LoadDevAsync(StorageTreeService tree)
    {
        var list = new List<UnifiedCandidate>();
        try
        {
            foreach (var a in await DevStorageService.DiscoverArtifactsAsync(tree))
            {
                // Only dormant artifacts that the Developer page also offers a direct clean for.
                if (!a.IsDormant || a.RiskLevel == RiskLevel.High || a.SizeBytes == 0) continue;

                list.Add(new UnifiedCandidate
                {
                    Source = CandidateSource.DevArtifact,
                    Name = $"{a.ProjectName} · {a.Name}",
                    Description = $"{a.Description} ({a.DormancyText})",
                    PathDisplay = a.Path,
                    Bytes = a.SizeBytes,
                    FileCount = a.FileCount,
                    Risk = a.RiskLevel,
                    IsProtected = CleanupService.IsPathProtected(a.Path),
                    Path = a.Path,
                });
            }
            return (list, null);
        }
        catch (Exception ex)
        {
            return (list, $"Developer storage analysis failed: {ex.Message}");
        }
    }

    private static async Task<(List<UnifiedCandidate>, string?)> LoadLeftoversAsync(StorageTreeService tree)
    {
        var list = new List<UnifiedCandidate>();
        try
        {
            var apps = await AppManagerService.DiscoverApplicationsAsync(tree);
            foreach (var l in await AppManagerService.DiscoverLeftoversAsync(apps, tree))
            {
                if (l.SizeBytes == 0) continue;

                list.Add(new UnifiedCandidate
                {
                    Source = CandidateSource.AppLeftover,
                    Name = $"{l.AppName} leftover",
                    Description = $"{l.LocationTypeText} residue of an application that is no longer installed",
                    PathDisplay = l.Path,
                    Bytes = l.SizeBytes,
                    FileCount = l.FileCount,
                    Risk = l.RiskLevel,
                    IsProtected = CleanupService.IsPathProtected(l.Path),
                    Path = l.Path,
                });
            }
            return (list, null);
        }
        catch (Exception ex)
        {
            return (list, $"App leftover analysis failed: {ex.Message}");
        }
    }

    /// <summary>
    /// Cleans every selected, unprotected candidate. Dry-run only counts. <paramref name="sendToRecycleBin"/>
    /// applies to system rules and dev artifacts; app leftovers always go to the Recycle Bin.
    /// </summary>
    public static async Task<CleanupRunReport> ExecuteAsync(
        StorageTreeService tree,
        IEnumerable<UnifiedCandidate> selected,
        bool dryRun,
        bool sendToRecycleBin)
    {
        ulong files = 0, bytes = 0, filesFailed = 0;
        int itemsFailed = 0;
        string? firstError = null;

        foreach (var item in selected.Where(c => c.IsSelected && !c.IsProtected))
        {
            try
            {
                switch (item.Source)
                {
                    case CandidateSource.SystemJunk when item.RuleId.HasValue:
                    {
                        var r = await tree.ExecuteCleanupRuleAsync(item.RuleId.Value, dryRun, sendToRecycleBin);
                        files += r.FilesReclaimed;
                        bytes += r.BytesReclaimed;
                        filesFailed += r.FilesFailed;
                        break;
                    }
                    case CandidateSource.DevArtifact when item.Path != null:
                    {
                        var r = await DevStorageService.CleanArtifactAsync(item.Path, dryRun, sendToRecycleBin);
                        files += r.FilesReclaimed;
                        bytes += r.BytesReclaimed;
                        filesFailed += r.FilesFailed;
                        break;
                    }
                    case CandidateSource.AppLeftover when item.Path != null:
                    {
                        if (CleanupService.IsPathProtected(item.Path))
                        {
                            itemsFailed++;
                            firstError ??= $"Skipped protected path {item.Path}";
                            break;
                        }

                        if (!Directory.Exists(item.Path)) break;

                        if (!dryRun)
                        {
                            // Never a permanent delete for leftovers.
                            await Task.Run(() => Microsoft.VisualBasic.FileIO.FileSystem.DeleteDirectory(
                                item.Path,
                                Microsoft.VisualBasic.FileIO.UIOption.OnlyErrorDialogs,
                                Microsoft.VisualBasic.FileIO.RecycleOption.SendToRecycleBin));
                        }
                        files += item.FileCount;
                        bytes += item.Bytes;
                        break;
                    }
                }
            }
            catch (Exception ex)
            {
                itemsFailed++;
                firstError ??= $"{item.Name}: {ex.Message}";
            }
        }

        return new CleanupRunReport(files, bytes, filesFailed, itemsFailed, firstError);
    }
}
