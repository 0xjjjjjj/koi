#requires -Version 5.0
# koi Windows visual smoke test
#
# Launches koi.exe (from -KoiPath, or koi.exe next to this script, or the
# repo's target\release\koi.exe), waits ~5 seconds for the window to
# render, then grabs a screenshot of the primary screen and saves it
# next to this script as koi-smoke-<yyyyMMdd-HHmmss>.png.
#
# Usage:
#   PS> .\windows-smoke.ps1                            # auto-locate koi.exe
#   PS> .\windows-smoke.ps1 -KoiPath C:\path\to\koi.exe

[CmdletBinding()]
param(
    [string] $KoiPath,
    [int]    $SleepSeconds = 5
)

$ErrorActionPreference = 'Stop'

if (-not $KoiPath) {
    $candidates = @(
        (Join-Path $PSScriptRoot 'koi.exe'),
        (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\koi.exe')
    )
    $KoiPath = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $KoiPath) {
        throw "koi.exe not found next to this script or at target\release\koi.exe. Pass -KoiPath explicitly."
    }
}
elseif (-not (Test-Path -LiteralPath $KoiPath)) {
    throw "koi.exe not found at '$KoiPath'."
}

Write-Host "Launching $KoiPath ..."
$proc = Start-Process -FilePath $KoiPath -PassThru
Write-Host "Started PID $($proc.Id). Waiting $SleepSeconds seconds for window to render..."
Start-Sleep -Seconds $SleepSeconds

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bitmap = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($screen.Location, [System.Drawing.Point]::Empty, $screen.Size)

$stamp    = Get-Date -Format 'yyyyMMdd-HHmmss'
$outPath  = Join-Path $PSScriptRoot ("koi-smoke-$stamp.png")
$bitmap.Save($outPath, [System.Drawing.Imaging.ImageFormat]::Png)

$graphics.Dispose()
$bitmap.Dispose()

Write-Host "Saved screenshot: $outPath"
Write-Host ""
Write-Host "koi is still running (PID $($proc.Id))."
Write-Host "Continue your manual checks in the window (dir / dark-mode toggle / Ctrl+C)."
Write-Host "Close the koi window when you are done, or run: Stop-Process -Id $($proc.Id)"
