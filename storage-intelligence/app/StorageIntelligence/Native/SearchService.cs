using System;
using System.Threading.Tasks;

namespace StorageIntelligence.Native;

/// <summary>
/// A single ranked search result item (ADR-012).
/// </summary>
public sealed record SearchResultItem(
    ulong NodeId,
    string Name,
    string FullPath,
    ulong Size,
    NodeKind Kind,
    FileCategory Category,
    uint Score
);

/// <summary>
/// Service for executing in-memory filtered queries on StorageTree (ADR-012).
/// </summary>
public static unsafe class SearchService
{
    /// <summary>
    /// Executes a search query synchronously over the active in-memory StorageTree.
    /// </summary>
    public static SearchResultItem[] Search(
        StorageTreeService tree,
        string query,
        uint maxResults = 100)
    {
        if (tree == null) throw new ArgumentNullException(nameof(tree));
        if (maxResults == 0) return Array.Empty<SearchResultItem>();

        query ??= string.Empty;
        var buffer = new SearchResultFfi[maxResults];
        uint count = 0;

        fixed (char* queryPtr = query)
        fixed (SearchResultFfi* resultsPtr = buffer)
        {
            int code = NativeMethods.tree_search(
                tree.Handle.DangerousHandle,
                (ushort*)queryPtr,
                maxResults,
                resultsPtr,
                &count);

            if (code != 0)
            {
                throw new ScanException(code, $"Failed to execute search with query '{query}'");
            }
        }

        var results = new SearchResultItem[count];
        for (int i = 0; i < count; i++)
        {
            var ffi = buffer[i];
            string name = string.Empty;
            string fullPath = string.Empty;

            try
            {
                name = tree.GetNodeName(ffi.node_id);
                fullPath = tree.GetFullPath(ffi.node_id);
            }
            catch
            {
                name = $"node_{ffi.node_id}";
                fullPath = name;
            }

            results[i] = new SearchResultItem(
                NodeId: ffi.node_id,
                Name: name,
                FullPath: fullPath,
                Size: ffi.size,
                Kind: (NodeKind)ffi.kind,
                Category: (FileCategory)ffi.category,
                Score: ffi.score
            );
        }

        return results;
    }

    /// <summary>
    /// Executes a search query asynchronously on a background thread.
    /// </summary>
    public static Task<SearchResultItem[]> SearchAsync(
        StorageTreeService tree,
        string query,
        uint maxResults = 100)
    {
        return Task.Run(() => Search(tree, query, maxResults));
    }
}
