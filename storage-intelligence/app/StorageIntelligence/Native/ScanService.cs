using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>Scan totals (ADR-004). Per-item detail is a Phase 3 (Storage Tree) concern.</summary>
public readonly record struct ScanSummaryInfo(ulong TotalFiles, ulong TotalDirs, ulong TotalSize, ulong InaccessibleCount);

public readonly record struct ScanProgressInfo(ulong FilesScanned, ulong BytesScanned);

/// <summary>Thrown when the native scan_drive call returns a non-zero error code (ADR-004).</summary>
public sealed class ScanException : Exception
{
    public int ErrorCode { get; }

    public ScanException(int errorCode, string message) : base(message) => ErrorCode = errorCode;
}

internal sealed unsafe class CancelTokenSafeHandle : SafeHandle
{
    public CancelTokenSafeHandle() : base(IntPtr.Zero, ownsHandle: true)
    {
        SetHandle((IntPtr)NativeMethods.cancel_token_create());
    }

    public override bool IsInvalid => handle == IntPtr.Zero;

    public void Cancel() => NativeMethods.cancel_token_cancel((CancelHandle*)handle);

    protected override bool ReleaseHandle()
    {
        NativeMethods.cancel_token_destroy((CancelHandle*)handle);
        return true;
    }
}

public sealed unsafe class ScanResultSafeHandle : SafeHandle
{
    public ScanResultSafeHandle(IntPtr rawHandle) : base(IntPtr.Zero, ownsHandle: true)
    {
        SetHandle(rawHandle);
    }

    public override bool IsInvalid => handle == IntPtr.Zero;

    internal ScanResultHandle* DangerousHandle => (ScanResultHandle*)handle;

    internal ScanSummaryFfi ReadSummary() => NativeMethods.scan_result_summary((ScanResultHandle*)handle);

    public ScanSummaryInfo ReadSummaryInfo()
    {
        var summary = ReadSummary();
        return new ScanSummaryInfo(
            summary.total_files, summary.total_dirs, summary.total_size, summary.inaccessible_count);
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.scan_result_destroy((ScanResultHandle*)handle);
        return true;
    }
}

/// <summary>
/// C#-facing wrapper over the native scanner (ADR-001/007): exposes an idiomatic
/// Task-based async API backed by a single blocking P/Invoke call run on a background
/// thread pool thread, never the UI thread.
/// </summary>
public static unsafe class ScanService
{
    private sealed class ProgressContext
    {
        public IProgress<ScanProgressInfo>? Progress;
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void OnProgress(ScanProgressFfi progress, void* userData)
    {
        var gch = GCHandle.FromIntPtr((IntPtr)userData);
        if (gch.Target is ProgressContext ctx)
        {
            ctx.Progress?.Report(new ScanProgressInfo(progress.files_scanned, progress.bytes_scanned));
        }
    }

    /// <summary>
    /// Scans <paramref name="path"/> on a background thread. Cancelling
    /// <paramref name="cancellationToken"/> requests native cancellation (ADR-007); the
    /// task then completes with a <see cref="ScanException"/> (native code: Cancelled).
    /// </summary>
    public static Task<ScanSummaryInfo> ScanDriveAsync(
        string path,
        CancellationToken cancellationToken = default,
        IProgress<ScanProgressInfo>? progress = null)
    {
        // Deliberately NOT passed to Task.Run's own cancellation parameter: that would make
        // Task.Run short-circuit before the delegate ever runs when already cancelled,
        // bypassing native cancellation entirely and throwing TaskCanceledException instead
        // of the intended ScanException(Cancelled). The token is instead registered inside
        // ScanDriveBlocking so the native path always observes and reports cancellation.
        return Task.Run(() => ScanDriveBlocking(path, cancellationToken, progress));
    }

    /// <summary>
    /// Scans <paramref name="path"/> on a background thread and returns the owned
    /// <see cref="ScanResultSafeHandle"/> containing summary and scan events.
    /// The caller is responsible for disposing the handle or passing it to
    /// <c>StorageTreeService.BuildAsync</c> (ADR-009 #5).
    /// </summary>
    public static Task<ScanResultSafeHandle> ScanDriveWithResultAsync(
        string path,
        CancellationToken cancellationToken = default,
        IProgress<ScanProgressInfo>? progress = null)
    {
        return Task.Run(() => ScanDriveResultBlocking(path, cancellationToken, progress));
    }

    private static ScanSummaryInfo ScanDriveBlocking(
        string path,
        CancellationToken cancellationToken,
        IProgress<ScanProgressInfo>? progress)
    {
        using var resultHandle = ScanDriveResultBlocking(path, cancellationToken, progress);
        return resultHandle.ReadSummaryInfo();
    }

    public static ScanResultSafeHandle ScanDriveResultBlocking(
        string path,
        CancellationToken cancellationToken,
        IProgress<ScanProgressInfo>? progress)
    {
        using var cancelHandle = new CancelTokenSafeHandle();
        using var registration = cancellationToken.Register(
            static state => ((CancelTokenSafeHandle)state!).Cancel(), cancelHandle);

        var context = new ProgressContext { Progress = progress };
        var gch = GCHandle.Alloc(context);
        try
        {
            fixed (char* pathPtr = path)
            {
                ScanResultHandle* rawResult = null;
                ushort* errorMessage = null;

                int code = NativeMethods.scan_drive(
                    (ushort*)pathPtr,
                    (CancelHandle*)cancelHandle.DangerousGetHandle(),
                    &OnProgress,
                    (void*)GCHandle.ToIntPtr(gch),
                    &rawResult,
                    &errorMessage);

                if (code != 0)
                {
                    string message = errorMessage != null ? new string((char*)errorMessage) : "unknown scan error";
                    if (errorMessage != null)
                    {
                        NativeMethods.free_error_message(errorMessage);
                    }
                    throw new ScanException(code, message);
                }

                return new ScanResultSafeHandle((IntPtr)rawResult);
            }
        }
        finally
        {
            gch.Free();
        }
    }
}
