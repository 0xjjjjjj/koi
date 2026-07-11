<#
.SYNOPSIS
    Build a distributable Windows release of koi.

.DESCRIPTION
    Produces target/release/koi.exe with the MSVC C runtime linked
    statically (equivalent to /MT), so the resulting binary does not
    require an installed Visual C++ redistributable on the target
    machine.

    With -Zip, also emits target/koi-windows-x86_64.zip containing the
    exe plus README for portable distribution.

.PARAMETER Zip
    After building, package the exe and top-level docs into a portable
    zip at target/koi-windows-x86_64.zip.

.EXAMPLE
    pwsh -File bundle/windows/build-windows.ps1
    pwsh -File bundle/windows/build-windows.ps1 -Zip

.NOTES
    Requires the x86_64-pc-windows-msvc Rust toolchain and MSVC build
    tools (Visual Studio Build Tools with the "Desktop development with
    C++" workload). Run from the repository root.
#>
[CmdletBinding()]
param(
    [switch]$Zip
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Push-Location $repoRoot
try {
    Write-Host "==> Building koi.exe (release, +crt-static)"
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    $exe = Join-Path $repoRoot 'target/release/koi.exe'
    if (-not (Test-Path $exe)) { throw "expected artifact not found: $exe" }
    Write-Host "==> Built $exe"

    if ($Zip) {
        $stage = Join-Path $repoRoot 'target/koi-windows-x86_64'
        $zip   = Join-Path $repoRoot 'target/koi-windows-x86_64.zip'
        if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
        if (Test-Path $zip)   { Remove-Item -Force $zip }
        New-Item -ItemType Directory -Path $stage | Out-Null

        Copy-Item $exe (Join-Path $stage 'koi.exe')
        foreach ($doc in 'README.md') {
            $src = Join-Path $repoRoot $doc
            if (Test-Path $src) { Copy-Item $src $stage }
        }

        Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
        Write-Host "==> Packaged $zip"
    }
}
finally {
    Pop-Location
}
