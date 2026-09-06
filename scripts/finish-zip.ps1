$ErrorActionPreference = "Stop"
Get-Process | Where-Object { $_.ProcessName -like "nocheater*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "package.json"))) {
  $root = "C:\Users\a\Desktop\nocheater-nfa-loader"
}

$dist = Join-Path $root "dist"
$portable = Join-Path $dist "nocheater-NFA-Loader-portable"
$desktop = [Environment]::GetFolderPath("Desktop")
$zipPath = Join-Path $dist "nocheater-NFA-Loader-portable.zip"
$deskZip = Join-Path $desktop "nocheater-NFA-Loader-portable.zip"

if (Test-Path $zipPath) { Remove-Item -Force $zipPath -ErrorAction SilentlyContinue }
if (Test-Path $deskZip) { Remove-Item -Force $deskZip -ErrorAction SilentlyContinue }

Push-Location $portable
& tar -a -cf $zipPath *
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "tar zip failed: $LASTEXITCODE" }
Pop-Location
$global:LASTEXITCODE = 0

if (-not (Test-Path $zipPath)) { throw "zip missing after tar" }
Copy-Item $zipPath $deskZip -Force
Write-Host ("ZIP OK bytes=" + (Get-Item $zipPath).Length)
Write-Host ("Desktop zip: " + $deskZip)

$checks = @(
  (Join-Path $desktop "nocheater NFA Loader_0.1.1_x64-setup.exe"),
  (Join-Path $desktop "nocheater-NFA-Loader-portable\nocheater NFA Loader.exe"),
  $deskZip
)
foreach ($c in $checks) {
  if (Test-Path -LiteralPath $c) {
    $i = Get-Item -LiteralPath $c
    Write-Host ("OK " + $c + " (" + $i.Length + ")")
  } else {
    Write-Host ("MISSING " + $c)
  }
}
