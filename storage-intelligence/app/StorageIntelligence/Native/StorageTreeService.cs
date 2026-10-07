using System;
using System.Runtime.InteropServices;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>
/// Node category in the storage tree (ADR-009).
/// </summary>
public enum NodeKind
{
    Directory = 0,
    File = 1,
    ReparsePoint = 2,
    Inaccessible = 3,
}

/// <summary>
/// Managed view of a single tree node (ADR-009).
/// </summary>
public readonly record struct NodeInfo(
    ulong Id,
    ulong? ParentId,
    NodeKind Kind,
    ulong Size,
    ulong FileCount,
    ulong DirCount,
    string Name,
    string? FullPath = null
);

/// <summary>
/// Paginated child query result (ADR-001 §2).
/// </summary>
public readonly record struct ChildrenResult(
    ulong[] NodeIds,
    uint TotalChildren
);

/// <summary>
/// SafeHandle wrapping a native <c>TreeHandle*</c> (ADR-009).
/// Disposing this releases the native arena in Rust memory.
/// </summary>
public sealed unsafe class TreeSafeHandle : SafeHandle
{
    public TreeSafeHandle(IntPtr rawHandle) : base(IntPtr.Zero, ownsHandle: true)
    {
        SetHandle(rawHandle);
    }

    public override bool IsInvalid => handle == IntPtr.Zero;

    internal TreeHandle* DangerousHandle => (TreeHandle*)handle;

    protected override bool ReleaseHandle()
    {
        NativeMethods.tree_destroy((TreeHandle*)handle);
        return true;
    }
}

/// <summary>
/// C#-facing wrapper over the native StorageTree (ADR-009): exposes safe, idiomatic
/// query methods over the immutable in-memory arena in Rust.
/// </summary>
public sealed unsafe class StorageTreeService : IDisposable
{
    private readonly TreeSafeHandle _handle;
    private bool _disposed;

    public TreeSafeHandle Handle => _handle;

    public StorageTreeService(TreeSafeHandle handle)
    {
        _handle = handle ?? throw new ArgumentNullException(nameof(handle));
        if (_handle.IsInvalid)
        {
            throw new ArgumentException("Invalid tree handle", nameof(handle));
        }
    }

    /// <summary>
    /// Builds a <see cref="StorageTreeService"/> asynchronously on a background thread
    /// from the completed <see cref="ScanResultSafeHandle"/>. The scan events are moved
    /// out of the scan result (ADR-009 #5).
    /// </summary>
    public static Task<StorageTreeService> BuildAsync(ScanResultSafeHandle scanResult)
    {
        return Task.Run(() => Build(scanResult));
    }

    /// <summary>
    /// Builds a <see cref="StorageTreeService"/> synchronously from <paramref name="scanResult"/>.
    /// </summary>
    public static StorageTreeService Build(ScanResultSafeHandle scanResult)
    {
        if (scanResult == null)
        {
            throw new ArgumentNullException(nameof(scanResult));
        }
        if (scanResult.IsInvalid)
        {
            throw new ArgumentException("Invalid scan result handle", nameof(scanResult));
        }

        TreeHandle* rawTree = null;
        ushort* errorMessage = null;

        int code = NativeMethods.tree_create(
            scanResult.DangerousHandle,
            &rawTree,
            &errorMessage);

        if (code != 0)
        {
            string message = errorMessage != null ? new string((char*)errorMessage) : "failed to build storage tree";
            if (errorMessage != null)
            {
                NativeMethods.free_error_message(errorMessage);
            }
            throw new ScanException(code, message);
        }

        return new StorageTreeService(new TreeSafeHandle((IntPtr)rawTree));
    }

    /// <summary>
    /// Gets the root node ID of the storage tree.
    /// </summary>
    public ulong GetRootId()
    {
        ThrowIfDisposed();
        return NativeMethods.tree_root_id(_handle.DangerousHandle);
    }

    /// <summary>
    /// Gets the total number of nodes in the tree.
    /// </summary>
    public ulong GetNodeCount()
    {
        ThrowIfDisposed();
        return NativeMethods.tree_node_count(_handle.DangerousHandle);
    }

    /// <summary>
    /// Gets metadata for a single node by its ID.
    /// </summary>
    public NodeInfo GetNodeInfo(ulong nodeId, bool includeFullPath = false)
    {
        ThrowIfDisposed();
        NodeInfoFfi ffiInfo = default;
        int code = NativeMethods.tree_node_info(_handle.DangerousHandle, nodeId, &ffiInfo);
        if (code != 0)
        {
            throw new ScanException(code, $"Node {nodeId} not found");
        }

        string name = GetNodeName(nodeId);
        string? fullPath = includeFullPath ? GetFullPath(nodeId) : null;

        return new NodeInfo(
            ffiInfo.id,
            ffiInfo.parent_id == ulong.MaxValue ? null : ffiInfo.parent_id,
            (NodeKind)ffiInfo.kind,
            ffiInfo.size,
            ffiInfo.file_count,
            ffiInfo.dir_count,
            name,
            fullPath);
    }

    /// <summary>
    /// Gets the name of a node.
    /// </summary>
    public string GetNodeName(ulong nodeId)
    {
        ThrowIfDisposed();
        Span<char> buffer = stackalloc char[260];
        uint actualLen = 0;
        fixed (char* bufPtr = buffer)
        {
            int code = NativeMethods.tree_node_name(
                _handle.DangerousHandle, nodeId, (ushort*)bufPtr, (uint)buffer.Length, &actualLen);
            if (code == 0)
            {
                return new string(buffer[..(int)actualLen]);
            }
        }

        if (actualLen > 0)
        {
            var heapBuf = new char[actualLen + 1];
            fixed (char* bufPtr = heapBuf)
            {
                int code = NativeMethods.tree_node_name(
                    _handle.DangerousHandle, nodeId, (ushort*)bufPtr, (uint)heapBuf.Length, &actualLen);
                if (code == 0)
                {
                    return new string(heapBuf, 0, (int)actualLen);
                }
            }
        }

        throw new ScanException(1, $"Failed to get name for node {nodeId}");
    }

    /// <summary>
    /// Reconstructs the full path of a node by walking ancestors to the root.
    /// </summary>
    public string GetFullPath(ulong nodeId)
    {
        ThrowIfDisposed();
        Span<char> buffer = stackalloc char[1024];
        uint actualLen = 0;
        fixed (char* bufPtr = buffer)
        {
            int code = NativeMethods.tree_node_path(
                _handle.DangerousHandle, nodeId, (ushort*)bufPtr, (uint)buffer.Length, &actualLen);
            if (code == 0)
            {
                return new string(buffer[..(int)actualLen]);
            }
        }

        if (actualLen > 0)
        {
            var heapBuf = new char[actualLen + 1];
            fixed (char* bufPtr = heapBuf)
            {
                int code = NativeMethods.tree_node_path(
                    _handle.DangerousHandle, nodeId, (ushort*)bufPtr, (uint)heapBuf.Length, &actualLen);
                if (code == 0)
                {
                    return new string(heapBuf, 0, (int)actualLen);
                }
            }
        }

        throw new ScanException(1, $"Failed to get full path for node {nodeId}");
    }

    /// <summary>
    /// Searches for a node by absolute filesystem path within the active StorageTree.
    /// Returns the NodeInfo if found, or null if not present in the tree.
    /// </summary>
    public NodeInfo? FindByPath(string path)
    {
        ThrowIfDisposed();
        if (string.IsNullOrWhiteSpace(path)) return null;

        var widePath = (path + "\0").ToCharArray();
        ulong nodeId = 0;
        fixed (char* pathPtr = widePath)
        {
            int code = NativeMethods.tree_find_by_path(
                _handle.DangerousHandle,
                (ushort*)pathPtr,
                &nodeId);

            if (code == 0)
            {
                return GetNodeInfo(nodeId, includeFullPath: true);
            }
        }
        return null;
    }

    /// <summary>
    /// Gets children of a directory node (pre-sorted by size descending), paginated via offset/limit.
    /// </summary>
    public ChildrenResult GetChildren(ulong nodeId, uint offset, uint limit)
    {
        ThrowIfDisposed();
        if (limit == 0)
        {
            return new ChildrenResult(Array.Empty<ulong>(), 0);
        }

        var ids = new ulong[limit];
        uint count = 0;
        uint total = 0;

        fixed (ulong* idsPtr = ids)
        {
            int code = NativeMethods.tree_children(
                _handle.DangerousHandle, nodeId, offset, limit, idsPtr, &count, &total);
            if (code != 0)
            {
                throw new ScanException(code, $"Failed to get children for node {nodeId}");
            }
        }

        if (count < limit)
        {
            Array.Resize(ref ids, (int)count);
        }

        return new ChildrenResult(ids, total);
    }

    /// <summary>
    /// Gets ancestors of a node bottom-up (node → parent → ... → root).
    /// </summary>
    public ulong[] GetAncestors(ulong nodeId, uint limit = 128)
    {
        ThrowIfDisposed();
        if (limit == 0)
        {
            return Array.Empty<ulong>();
        }

        var ids = new ulong[limit];
        uint count = 0;

        fixed (ulong* idsPtr = ids)
        {
            int code = NativeMethods.tree_ancestors(
                _handle.DangerousHandle, nodeId, idsPtr, limit, &count);
            if (code != 0)
            {
                throw new ScanException(code, $"Failed to get ancestors for node {nodeId}");
            }
        }

        if (count < limit)
        {
            Array.Resize(ref ids, (int)count);
        }

        return ids;
    }

    /// <summary>
    /// Gets the top N largest files within the subtree rooted at <paramref name="rootId"/>.
    /// </summary>
    public ulong[] GetTopFilesBySize(ulong rootId, uint limit)
    {
        ThrowIfDisposed();
        if (limit == 0)
        {
            return Array.Empty<ulong>();
        }

        var ids = new ulong[limit];
        uint count = 0;

        fixed (ulong* idsPtr = ids)
        {
            int code = NativeMethods.tree_top_files_by_size(
                _handle.DangerousHandle, rootId, limit, idsPtr, &count);
            if (code != 0)
            {
                throw new ScanException(code, $"Failed to get top files for root {rootId}");
            }
        }

        if (count < limit)
        {
            Array.Resize(ref ids, (int)count);
        }

        return ids;
    }

    /// <summary>
    /// Computes squarified treemap layout starting from <paramref name="rootId"/> for the given viewport.
    /// </summary>
    public TreemapRect[] ComputeLayout(ulong rootId, float width, float height, uint maxDepth = 2, float minSizePx = 3.0f)
    {
        ThrowIfDisposed();
        return TreemapService.ComputeLayout(this, rootId, width, height, maxDepth, minSizePx);
    }

    /// <summary>
    /// Searches the tree using a query string and returns ranked top results (ADR-012).
    /// </summary>
    public SearchResultItem[] Search(string query, uint maxResults = 100)
    {
        ThrowIfDisposed();
        return SearchService.Search(this, query, maxResults);
    }

    /// <summary>
    /// Asynchronously searches the tree on a threadpool worker (ADR-012).
    /// </summary>
    public Task<SearchResultItem[]> SearchAsync(string query, uint maxResults = 100)
    {
        ThrowIfDisposed();
        return SearchService.SearchAsync(this, query, maxResults);
    }

    /// <summary>
    /// Detects all cleanup candidates across the active tree and system locations (ADR-013).
    /// </summary>
    public Task<CleanupCandidateInfo[]> DetectCleanupCandidatesAsync()
    {
        ThrowIfDisposed();
        return CleanupService.DetectCandidatesAsync(this);
    }

    /// <summary>
    /// Executes or simulates safe cleanup for a candidate rule category (ADR-013).
    /// </summary>
    public Task<CleanupExecutionReport> ExecuteCleanupRuleAsync(CleanupRuleId ruleId, bool dryRun, bool sendToRecycleBin = true)
    {
        ThrowIfDisposed();
        return CleanupService.ExecuteRuleAsync(this, ruleId, dryRun, sendToRecycleBin);
    }

    private void ThrowIfDisposed()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
    }

    public void Dispose()
    {
        if (!_disposed)
        {
            _handle.Dispose();
            _disposed = true;
        }
    }
}
