using System;
using System.IO;
using System.Runtime.CompilerServices;

namespace StorageIntelligence.IntegrationTests;

/// <summary>
/// Appends timestamped progress lines to the file named by SI_TEST_TRACE (set in CI), so a test
/// that stalls on a runner shows exactly which step it reached. No-op when the variable is unset.
/// </summary>
internal static class TestTrace
{
    private static readonly string? TracePath = Environment.GetEnvironmentVariable("SI_TEST_TRACE");
    private static readonly object Lock = new();

    public static void Step(string message, [CallerMemberName] string test = "")
    {
        if (string.IsNullOrEmpty(TracePath)) return;
        lock (Lock)
        {
            File.AppendAllText(TracePath, $"{DateTime.UtcNow:HH:mm:ss.fff} {test}: {message}{Environment.NewLine}");
        }
    }
}
