# SPDX-License-Identifier: MPL-2.0
# BOS-31: one BattalionOS DEVELOPER build, run directly on this PC, with an app taken from its folder.
#
#   scripts/bos-dev.ps1 -App <folder> [-AppUrl <loopback url>] [-OsWatch]
#
# - Builds the OS with the `dev` feature (never a release build) and its own identity (single-instance id org.battalionos.desktop.dev),
#   so it runs BESIDE an installed BattalionOS without closing, focusing or replacing it.
# - Starts the dev server of the OS's own screens (crates/battalion-desktop/ui) on a free port of this PC's loopback and shows the window
#   from it: a change in the UI is seen live. The OS Rust code is only rebuilt when it changes (-OsWatch watches it).
# - Gives the OS and the app their own data folder (<DevRoot>) and runs the OS with HOME, USERPROFILE, APPDATA and LOCALAPPDATA there, so
#   nothing of the owner's tools or the installed apps is read or changed. The OS's BATTALION_*, INFANTRY_*, AGENTHQ_* variables are removed.
# - Listens on 127.0.0.1 only. No firewall rule. Stops by pid (`-Stop`); never by image name.
param(
  [string[]]$App = @(),
  [string[]]$AppUrl = @(),
  [string[]]$AppEnv = @(),   # KEY=VALUE for the LAST app (e.g. a free port for its own server)
  [string]$DevRoot = (Join-Path $env:TEMP 'bos-dev'),
  [string]$Open = '',          # an app id the OS window opens at start (its `--open <id>`)
  [switch]$OsWatch,
  [switch]$Stop
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$ui = Join-Path $repo 'crates\battalion-desktop\ui'
$pidFile = Join-Path $DevRoot 'pids.json'

function Stop-Pid([int]$p) { if ($p -and (Get-Process -Id $p -ErrorAction SilentlyContinue)) { & taskkill.exe /PID $p /T /F | Out-Null } }
if ($Stop) {
  if (Test-Path $pidFile) { $j = Get-Content $pidFile -Raw | ConvertFrom-Json; Stop-Pid $j.os; Stop-Pid $j.ui; Remove-Item $pidFile }
  return
}

function Free-Port { $l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0); $l.Start(); $p = $l.LocalEndpoint.Port; $l.Stop(); $p }
foreach ($d in 'os', 'home', 'roaming', 'local') { New-Item -ItemType Directory -Force (Join-Path $DevRoot $d) | Out-Null }
$uiPort = Free-Port

# 1. The dev server of the OS's own screens, on loopback only.
if (-not (Test-Path (Join-Path $ui 'node_modules'))) { & pnpm --dir $ui install --frozen-lockfile }
$psi = [Diagnostics.ProcessStartInfo]::new('cmd.exe', "/c pnpm exec vite --host 127.0.0.1 --port $uiPort --strictPort")
$psi.WorkingDirectory = $ui; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
$psi.Environment['BOS_UI_PORT'] = "$uiPort"
$vite = [Diagnostics.Process]::Start($psi)
for ($i = 0; $i -lt 100; $i++) { try { $c = [Net.Sockets.TcpClient]::new('127.0.0.1', $uiPort); $c.Close(); break } catch { Start-Sleep -Milliseconds 200 } }

# 2. The OS: dev feature, its own identity, the window loads the dev server.
$env:TAURI_CONFIG = (@{ identifier = 'org.battalionos.desktop.dev'; build = @{ devUrl = "http://127.0.0.1:$uiPort" } } | ConvertTo-Json -Compress -Depth 5)
function Build-Os { Push-Location $repo; try { & cargo build -p battalion-desktop --features dev 2>&1 | Select-Object -Last 3; if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' } } finally { Pop-Location } }
function Start-Os {
  Push-Location $repo; try { $targetDir = (& cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory } finally { Pop-Location }
  $exe = Join-Path $targetDir 'debug\battalion-desktop.exe'
  $a = [Diagnostics.ProcessStartInfo]::new($exe); $a.UseShellExecute = $false; $a.WorkingDirectory = $repo
  foreach ($k in @($a.Environment.Keys)) { if ($k -match '^(BATTALION_|INFANTRY_|AGENTHQ_)') { $a.Environment.Remove($k) | Out-Null } }
  $a.Environment['BATTALION_HOME'] = Join-Path $DevRoot 'os'
  $a.Environment['HOME'] = Join-Path $DevRoot 'home'; $a.Environment['USERPROFILE'] = Join-Path $DevRoot 'home'
  $a.Environment['APPDATA'] = Join-Path $DevRoot 'roaming'; $a.Environment['LOCALAPPDATA'] = Join-Path $DevRoot 'local'
  for ($i = 0; $i -lt $App.Count; $i++) {
    $a.ArgumentList.Add('--dev-app'); $a.ArgumentList.Add((Resolve-Path $App[$i]).Path)
    if ($i -lt $AppUrl.Count -and $AppUrl[$i]) { $a.ArgumentList.Add('--dev-url'); $a.ArgumentList.Add($AppUrl[$i]) }
  }
  if ($Open) { $a.ArgumentList.Add('--open'); $a.ArgumentList.Add($Open) }
  foreach ($kv in $AppEnv) { $a.ArgumentList.Add('--dev-env'); $a.ArgumentList.Add($kv) }
  [Diagnostics.Process]::Start($a)
}
Build-Os
$os = Start-Os
@{ os = $os.Id; ui = $vite.Id; uiPort = $uiPort; devRoot = $DevRoot } | ConvertTo-Json | Set-Content $pidFile
Write-Host "BattalionOS dev: os pid $($os.Id), ui dev server http://127.0.0.1:$uiPort (pid $($vite.Id)), data $DevRoot"

# 3. A change in the OS's Rust code rebuilds the OS only (the app is not built here) and restarts the OS.
if ($OsWatch) {
  $src = Get-ChildItem (Join-Path $repo 'crates') -Recurse -Include *.rs, Cargo.toml, tauri.conf.json -File | Where-Object { $_.FullName -notmatch '\\(target|node_modules)\\' }
  $last = ($src | Measure-Object LastWriteTimeUtc -Maximum).Maximum
  while ($true) {
    Start-Sleep -Milliseconds 700
    $now = (Get-ChildItem (Join-Path $repo 'crates') -Recurse -Include *.rs, Cargo.toml, tauri.conf.json -File | Where-Object { $_.FullName -notmatch '\\(target|node_modules)\\' } | Measure-Object LastWriteTimeUtc -Maximum).Maximum
    if ($now -gt $last) {
      $last = $now; $t = Get-Date
      try { Build-Os } catch { Write-Host $_; continue }
      Stop-Pid $os.Id; $os = Start-Os
      @{ os = $os.Id; ui = $vite.Id; uiPort = $uiPort; devRoot = $DevRoot } | ConvertTo-Json | Set-Content $pidFile
      Write-Host ("OS rebuilt and restarted in {0:n1} s, pid {1}" -f ((Get-Date) - $t).TotalSeconds, $os.Id)
    }
  }
}
