<#
.SYNOPSIS
    Builds the Rust workspace (core/) and the WinUI 3 solution (app/).
    Native DLL -> C# output copy step will be added in Sub-phase 2.7 (FFI boundary),
    once the ffi crate actually exports a cdylib consumed by the app.
#>
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Debug"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

Write-Host "==> Building Rust workspace (core/)"
Push-Location (Join-Path $root "core")
try {
    if ($Configuration -eq "Release") {
        cargo build --workspace --release
    } else {
        cargo build --workspace
    }
} finally {
    Pop-Location
}

Write-Host "==> Building WinUI 3 solution (app/)"
Push-Location (Join-Path $root "app")
try {
    dotnet build StorageIntelligence.sln -c $Configuration
} finally {
    Pop-Location
}

Write-Host "==> Build complete"
