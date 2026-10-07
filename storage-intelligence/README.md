# Storage Intelligence

See the root [README.md](../../README.md) for full project documentation, architectural overview, and benchmarks.

## Quick Run

### Launch Built App

```powershell
& ".\app\StorageIntelligence\bin\Debug\net9.0-windows10.0.19041.0\win-x64\StorageIntelligence.exe"
```

### Run Tests

```powershell
# Rust Core (91 tests)
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cd core && cargo test --workspace

# .NET Integration Tests (30 tests)
$env:Path = "$env:USERPROFILE\.dotnet;$env:Path"
cd .. && dotnet test tests\integration\StorageIntelligence.IntegrationTests.csproj
```
