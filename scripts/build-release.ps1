# Build release installer + portable exe for nocheater NFA Loader (standalone, no Node)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "package.json"))) {
  $root = "C:\Users\a\Desktop\nocheater-nfa-loader"
}

$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
Set-Location $root

Write-Host "==> Regenerating peach-cat icons (app + NSIS)..."
python (Join-Path $root "scripts\gen_icons.py")

Write-Host "==> Killing running nocheater..."
Get-Process | Where-Object { $_.ProcessName -like "nocheater*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

Write-Host "==> tauri build (NSIS)..."
npm run tauri build
$global:LASTEXITCODE = 0

# Prefer newest release dir among CARGO_TARGET_DIR / src-tauri/target / temp cargo caches
$candidates = @()
if ($env:CARGO_TARGET_DIR) {
  $candidates += (Join-Path $env:CARGO_TARGET_DIR "release")
}
$candidates += (Join-Path $root "src-tauri\target\release")
$tempRoot = Join-Path $env:LOCALAPPDATA "Temp"
if (Test-Path $tempRoot) {
  Get-ChildItem $tempRoot -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $p = Join-Path $_.FullName "cargo-target\release"
    if (Test-Path (Join-Path $p "nocheater.exe")) { $candidates += $p }
  }
}

$releaseDir = $null
foreach ($c in $candidates) {
  $exeProbe = Join-Path $c "nocheater.exe"
  if (Test-Path $exeProbe) {
    if (-not $releaseDir) {
      $releaseDir = $c
    } else {
      $cur = Get-Item (Join-Path $releaseDir "nocheater.exe")
      $nex = Get-Item $exeProbe
      if ($nex.LastWriteTime -gt $cur.LastWriteTime) { $releaseDir = $c }
    }
  }
}
if (-not $releaseDir) { throw "Could not locate release nocheater.exe" }

$bundleNsis = Join-Path $releaseDir "bundle\nsis"
$dist = Join-Path $root "dist"
$portable = Join-Path $dist "nocheater-NFA-Loader-portable"
$desktop = [Environment]::GetFolderPath("Desktop")
$desktopPortable = Join-Path $desktop "nocheater-NFA-Loader-portable"

Write-Host ("==> Using release dir: " + $releaseDir)

New-Item -ItemType Directory -Force -Path $dist | Out-Null
if (Test-Path $portable) {
  cmd /c "rd /s /q `"$portable`"" | Out-Null
  if (Test-Path $portable) { Remove-Item -Recurse -Force $portable -ErrorAction SilentlyContinue }
}
New-Item -ItemType Directory -Force -Path $portable | Out-Null

$exe = Join-Path $releaseDir "nocheater.exe"
if (-not (Test-Path $exe)) { throw "Missing release exe: $exe" }

# Standalone portable: just the exe (no Node / checker sidecars)
Copy-Item $exe (Join-Path $portable "nocheater NFA Loader.exe") -Force

$setup = Get-ChildItem $bundleNsis -Filter "*.exe" -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $setup) { throw "NSIS installer not found in $bundleNsis" }

$setupPreferred = Get-ChildItem $bundleNsis -Filter "*0.1.1*.exe" -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if ($setupPreferred) { $setup = $setupPreferred }

Copy-Item $setup.FullName (Join-Path $dist $setup.Name) -Force
Copy-Item $setup.FullName (Join-Path $desktop $setup.Name) -Force

# Remove stale Desktop single-exe + old checker / portable layouts
Remove-Item (Join-Path $desktop "nocheater-NFA-Loader.exe") -Force -ErrorAction SilentlyContinue
$oldChecker = Join-Path $desktop "checker"
if (Test-Path $oldChecker) {
  if (Test-Path (Join-Path $oldChecker "validate-login.js")) {
    Remove-Item -Recurse -Force $oldChecker -ErrorAction SilentlyContinue
  }
}
Get-ChildItem $desktop -Filter "*0.1.0*setup*.exe" -ErrorAction SilentlyContinue | Remove-Item -Force

if (Test-Path $desktopPortable) { Remove-Item -Recurse -Force $desktopPortable }
robocopy $portable $desktopPortable /E /NFL /NDL /NJH /NJS /nc /ns /np | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy desktop portable failed" }
$global:LASTEXITCODE = 0

$zipPath = Join-Path $dist "nocheater-NFA-Loader-portable.zip"
if (Test-Path $zipPath) { Remove-Item -Force $zipPath }
Compress-Archive -Path (Join-Path $portable "*") -DestinationPath $zipPath -Force
Copy-Item $zipPath (Join-Path $desktop "nocheater-NFA-Loader-portable.zip") -Force

Write-Host ""
Write-Host "OK - artifacts:"
Write-Host ("  Installer: " + $setup.FullName)
Write-Host ("  Dist copy: " + (Join-Path $dist $setup.Name))
Write-Host ("  Desktop installer: " + (Join-Path $desktop $setup.Name))
Write-Host ("  Portable folder: " + $portable)
Write-Host ("  Desktop portable: " + $desktopPortable)
Write-Host ("  Portable zip: " + $zipPath)
Write-Host "Standalone exe — no Node, no checker resources required at runtime."
