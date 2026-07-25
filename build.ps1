[CmdletBinding()]
param(
    [ValidateSet("Debug", "Release")]
    [string]$Configuration = "Release"
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$profileDirectory = if ($Configuration -eq "Release") { "release" } else { "debug" }

Push-Location $root
try {
    if ($Configuration -eq "Release") {
        cargo build --workspace --release
    }
    else {
        cargo build --workspace
    }
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }

    $dist = Join-Path $root "dist"
    $mods = Join-Path $dist "NativeMods"
    New-Item -ItemType Directory -Force -Path $mods | Out-Null

    Copy-Item -Force `
        (Join-Path $root "target\$profileDirectory\dofus_native_host.dll") `
        (Join-Path $dist "DofusNativeHost.dll")
    Copy-Item -Force `
        (Join-Path $root "target\$profileDirectory\dofus_native_example.dll") `
        (Join-Path $mods "DofusNativeExample.dll")
    Copy-Item -Force `
        (Join-Path $root "target\$profileDirectory\dofus_native_network_dumper.dll") `
        (Join-Path $mods "DofusNetworkDumper.dll")
    Copy-Item -Force `
        (Join-Path $root "target\$profileDirectory\dofus_native_tactical.dll") `
        (Join-Path $mods "DofusNativeTactical.dll")
    Copy-Item -Force `
        (Join-Path $root "include\dofus_native_mod_api.h") `
        (Join-Path $dist "dofus_native_mod_api.h")

    $bootstrapBuild = Join-Path $root "target\bootstrap-direct"
    New-Item -ItemType Directory -Force -Path $bootstrapBuild | Out-Null
    $vswhere = @(
        (Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"),
        (Join-Path $env:ProgramFiles "Microsoft Visual Studio\Installer\vswhere.exe")
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($vswhere)) {
        throw "vswhere.exe was not found. Install Visual Studio Build Tools with Desktop development with C++."
    }

    $vsInstallationPath = & $vswhere `
        -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -latest `
        -property installationPath
    $vsInstallationPath = $vsInstallationPath | Select-Object -First 1
    if ([string]::IsNullOrWhiteSpace($vsInstallationPath)) {
        throw "No Visual Studio installation with the MSVC x64 toolset was found."
    }

    $vsDevShell = Join-Path $vsInstallationPath "Common7\Tools\Launch-VsDevShell.ps1"
    if (-not (Test-Path -LiteralPath $vsDevShell)) {
        throw "Developer PowerShell was not found at: $vsDevShell"
    }
    . $vsDevShell `
        -VsInstallationPath $vsInstallationPath `
        -Arch amd64 `
        -HostArch amd64 `
        -SkipAutomaticLocation

    $compiler = Get-ChildItem `
        -Path (Join-Path $vsInstallationPath "VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe") `
        -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
    if ([string]::IsNullOrWhiteSpace($compiler)) {
        throw "cl.exe x64 was not found below: $vsInstallationPath"
    }

    $bootstrapSource = Join-Path $root "bootstrap\src\version_proxy.cpp"
    $bootstrapDef = Join-Path $root "bootstrap\src\version_proxy.def"
    $bootstrapDll = Join-Path $bootstrapBuild "DofusNativeBootstrap.dll"
    & $compiler `
        /nologo `
        /std:c++20 `
        /EHsc `
        /O2 `
        /W4 `
        /WX `
        /permissive- `
        /guard:cf `
        /DNOMINMAX `
        /DWIN32_LEAN_AND_MEAN `
        /LD `
        "/Fo$(Join-Path $bootstrapBuild 'version_proxy.obj')" `
        $bootstrapSource `
        /link `
        "/DEF:$bootstrapDef" `
        "/OUT:$bootstrapDll" `
        "/IMPLIB:$(Join-Path $bootstrapBuild 'DofusNativeBootstrap.lib')" `
        "/PDB:$(Join-Path $bootstrapBuild 'DofusNativeBootstrap.pdb')" `
        /guard:cf
    if ($LASTEXITCODE -ne 0) {
        throw "bootstrap build failed with exit code $LASTEXITCODE"
    }

    $dofusRuntime = Join-Path $dist "DofusRuntime"
    $dofusMods = Join-Path $dofusRuntime "NativeMods"
    New-Item -ItemType Directory -Force -Path $dofusMods | Out-Null
    Copy-Item -Force $bootstrapDll (Join-Path $dofusRuntime "DofusNativeBootstrap.dll")
    Copy-Item -Force $bootstrapDll (Join-Path $dofusRuntime "version.dll")
    Copy-Item -Force `
        (Join-Path $dist "DofusNativeHost.dll") `
        (Join-Path $dofusRuntime "DofusNativeHost.dll")
    Copy-Item -Force `
        (Join-Path $mods "DofusNativeExample.dll") `
        (Join-Path $dofusMods "DofusNativeExample.dll")
    Copy-Item -Force `
        (Join-Path $mods "DofusNativeTactical.dll") `
        (Join-Path $dofusMods "DofusNativeTactical.dll")

    Write-Host "Build ready: $dist"
}
finally {
    Pop-Location
}
