[CmdletBinding()]
param(
    [string]$UnityPath
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($UnityPath)) {
    $UnityPath = Get-ChildItem `
        -Path "C:\Program Files\Unity\Hub\Editor" `
        -Recurse `
        -Filter "Unity.exe" `
        -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if ([string]::IsNullOrWhiteSpace($UnityPath) -or
    -not (Test-Path -LiteralPath $UnityPath)) {
    throw "Unity.exe was not found. Install Unity 6.0 LTS (6000.0.75f1) with Windows IL2CPP Build Support or pass -UnityPath."
}

& (Join-Path $root "build.ps1") -Configuration Release
if ($LASTEXITCODE -ne 0) {
    throw "Native runtime build failed with exit code $LASTEXITCODE"
}

& $UnityPath `
    -batchmode `
    -nographics `
    -quit `
    -projectPath (Join-Path $root "unity-harness") `
    -executeMethod "DofusNativeHarness.Editor.BuildHarness.BuildWindowsIl2Cpp" `
    -logFile "-"
if ($LASTEXITCODE -ne 0) {
    throw "Unity harness build failed with exit code $LASTEXITCODE"
}

Write-Host "Harness ready: $(Join-Path $root 'unity-harness\Build\Windows\DofusNativeHarness.exe')"
