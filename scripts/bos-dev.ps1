# SPDX-License-Identifier: MPL-2.0
# BOS-31 / DEV-POLISH-1: one BattalionOS DEVELOPER build, run directly on this PC, with an app taken from its folder.
#
#   scripts/bos-dev.ps1 -App <folder> [-AppUrl <loopback url>] [-OsWatch]     start (a second start while one runs says so)
#   scripts/bos-dev.ps1 -Stop                                                 stop everything this script started, by pid
#
# - Builds the OS with the `dev` feature (never a release build) into its OWN target folder (default %LOCALAPPDATA%\bos-dev-target\os, stated
#   at the end; a second start reuses it), never the checkout's normal target. Own identity (single-instance id org.battalionos.desktop.dev),
#   so it runs BESIDE an installed BattalionOS without closing, focusing or replacing it.
# - Starts the dev server of the OS's own screens (crates/battalion-desktop/ui) on a free loopback port: a change in the UI shows live.
#   -OsWatch also rebuilds and restarts the OS alone when its Rust changes. The OS runs from a COPY of its exe, so a rebuild is never blocked.
# - The OS, the app and every tool started here run with INFANTRY_*, AGENTHQ_* and BATTALION_* removed, and with HOME, USERPROFILE, APPDATA and
#   LOCALAPPDATA in <DevRoot>: nothing of the owner's tools or installed apps is read or changed. Listens on 127.0.0.1 only. No firewall rule.
# - Stops by pid (`-Stop`), never by image name. Logs: <DevRoot>\logs.
param(
  [string[]]$App = @(),
  [string[]]$AppUrl = @(),
  [string[]]$AppEnv = @(),   # KEY=VALUE for the LAST app (e.g. a free port for its own server)
  [string]$DevRoot = (Join-Path $env:TEMP 'bos-dev'),
  [string]$OsTargetDir = '',  # default: %LOCALAPPDATA%\bos-dev-target\os (its own folder, outside every checkout)
  [string]$Open = '',          # an app id the OS window opens at start (its `--open <id>`)
  [int]$MinFreeGb = 8,         # a debug OS build needs room: below this the start stops with one line
  [switch]$OsWatch,
  [switch]$Stop,
  [switch]$Quiet               # a caller (an app's dev script) prints the summary itself
)
$ErrorActionPreference = 'Stop'
# Everything started below inherits this: no installed-app, daemon or OS variable reaches cargo, pnpm, vite, the OS or the app.
Get-ChildItem Env: | Where-Object { $_.Name -match '^(INFANTRY_|AGENTHQ_|BATTALION_)' } | ForEach-Object { Remove-Item "Env:$($_.Name)" }
$repo = Split-Path $PSScriptRoot -Parent
$ui = Join-Path $repo 'crates\battalion-desktop\ui'
$pidFile = Join-Path $DevRoot 'pids.json'
$logs = Join-Path $DevRoot 'logs'
if (-not $OsTargetDir) { $OsTargetDir = Join-Path $env:LOCALAPPDATA 'bos-dev-target\os' }
function Fail([string]$why) { Write-Host "bos-dev: $why" -ForegroundColor Red; exit 1 }
# A pid counts as ours only with the start time we recorded (a reused pid is someone else's process).
function Test-Ours($p) { if (-not $p) { return $false }; $x = Get-Process -Id $p.id -ErrorAction SilentlyContinue; [bool]($x -and $x.StartTime.Ticks -eq [int64]$p.start) }
function Mark($proc) { [pscustomobject]@{ id = $proc.Id; start = (Get-Process -Id $proc.Id).StartTime.Ticks } }
function Stop-Ours($p) { if (Test-Ours $p) { & taskkill.exe /PID $p.id /T /F 2>&1 | Out-Null; return $true }; $false }

if ($Stop) {
  if (-not (Test-Path $pidFile)) { Write-Host 'bos-dev: nothing to stop (no dev OS was started from this data folder).'; return }
  $j = Get-Content $pidFile -Raw | ConvertFrom-Json
  $done = @()
  # The watcher first (it would restart the OS), then the OS, then the dev server.
  foreach ($k in 'watcher', 'os', 'ui') { if (Test-Ours $j.$k) { $done += $k } }
  foreach ($k in 'watcher', 'os', 'ui') { Stop-Ours $j.$k | Out-Null }
  Remove-Item $pidFile
  Write-Host ("bos-dev: stopped " + $(if ($done) { ($done -join ', ') } else { 'nothing (it had already ended)' }) + '. Its data stays in ' + $DevRoot + ' until you delete it.')
  return
}
if (Test-Path $pidFile) {
  $j = Get-Content $pidFile -Raw | ConvertFrom-Json
  if (Test-Ours $j.os) { Write-Host "bos-dev: a dev BattalionOS is already running (pid $($j.os.id)); not starting a second. To stop it: scripts/bos-dev.ps1 -Stop -DevRoot `"$DevRoot`""; return }
  Remove-Item $pidFile
}

# Plain checks first: one line each.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Fail 'cargo (Rust) is not installed or not on PATH. Install Rust from https://rustup.rs, open a new terminal, and run this again.' }
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) { Fail 'pnpm is not installed or not on PATH. Install it (npm install -g pnpm), open a new terminal, and run this again.' }
New-Item -ItemType Directory -Force $OsTargetDir | Out-Null
$drive = (Get-Item $OsTargetDir).PSDrive
$freeGb = [math]::Floor($drive.Free / 1GB)
if ($freeGb -lt $MinFreeGb) { Fail "only $freeGb GB free on $($drive.Name):, a dev build needs $MinFreeGb GB. Free some space (or delete $OsTargetDir) and run this again." }
foreach ($d in 'os', 'home', 'roaming', 'local', 'logs', 'run') { New-Item -ItemType Directory -Force (Join-Path $DevRoot $d) | Out-Null }
function Free-Port { $l = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0); $l.Start(); $p = $l.LocalEndpoint.Port; $l.Stop(); $p }
$uiPort = Free-Port
$t0 = Get-Date

# 1. The dev server of the OS's own screens, on loopback only.
if (-not (Test-Path (Join-Path $ui 'node_modules'))) {
  Write-Host 'Installing the OS screens packages (first start only)...'
  & pnpm --dir $ui install --frozen-lockfile *> (Join-Path $logs 'pnpm-install.log')
  if ($LASTEXITCODE -ne 0) { Fail "pnpm install failed (no network?). See $(Join-Path $logs 'pnpm-install.log'), fix it, and run this again." }
}
$psi = [Diagnostics.ProcessStartInfo]::new('cmd.exe', "/c pnpm exec vite --host 127.0.0.1 --port $uiPort --strictPort > `"$(Join-Path $logs 'ui.log')`" 2>&1")
$psi.WorkingDirectory = $ui; $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
$psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true   # the log files get the output; the terminal (or a pipe reading this script) is not held open by it
$psi.Environment['BOS_UI_PORT'] = "$uiPort"
$vite = [Diagnostics.Process]::Start($psi)
$up = $false
for ($i = 0; $i -lt 150 -and -not $up -and -not $vite.HasExited; $i++) { try { $c = [Net.Sockets.TcpClient]::new('127.0.0.1', $uiPort); $c.Close(); $up = $true } catch { Start-Sleep -Milliseconds 200 } }
if (-not $up) { & taskkill.exe /PID $vite.Id /T /F 2>&1 | Out-Null; Fail "the screens' dev server did not start on port $uiPort (taken, or a build error). Run this again; details: $(Join-Path $logs 'ui.log')" }

# 2. The OS: dev feature, its own identity and target folder; the window loads the dev server.
$env:TAURI_CONFIG = (@{ identifier = 'org.battalionos.desktop.dev'; build = @{ devUrl = "http://127.0.0.1:$uiPort" } } | ConvertTo-Json -Compress -Depth 5)
$env:CARGO_TARGET_DIR = $OsTargetDir
$built = Test-Path (Join-Path $OsTargetDir 'debug\battalion-desktop.exe')
function Build-Os {
  if ((Get-Item $OsTargetDir).PSDrive.Free / 1GB -lt $MinFreeGb) { throw "low disk (under $MinFreeGb GB free). Free some space and run this again." }
  Push-Location $repo
  try { & cargo build -p battalion-desktop --features dev *> (Join-Path $logs 'os-build.log'); if ($LASTEXITCODE -ne 0) { throw "the OS did not build; the errors are at the end of $(Join-Path $logs 'os-build.log')" } } finally { Pop-Location }
}
function Start-Os {
  # Run a COPY of the exe: cargo can then replace the built one while the OS runs.
  $exe = Join-Path $DevRoot 'run\battalion-desktop.exe'
  # A leftover copy still running (its launcher already gone) would lock the file: end it, by pid, by its own path under <DevRoot>.
  Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $exe } | ForEach-Object { & taskkill.exe /PID $_.ProcessId /T /F 2>&1 | Out-Null }
  Copy-Item (Join-Path $OsTargetDir 'debug\battalion-desktop.exe') $exe -Force
  $q = { param($s) '"' + $s + '"' }
  $argLine = ''
  for ($i = 0; $i -lt $App.Count; $i++) {
    $argLine += ' --dev-app ' + (& $q (Resolve-Path $App[$i]).Path)
    if ($i -lt $AppUrl.Count -and $AppUrl[$i]) { $argLine += ' --dev-url ' + (& $q $AppUrl[$i]) }
  }
  if ($Open) { $argLine += ' --open ' + (& $q $Open) }
  foreach ($kv in $AppEnv) { $argLine += ' --dev-env ' + (& $q $kv) }
  $a = [Diagnostics.ProcessStartInfo]::new('cmd.exe', '/c "' + (& $q $exe) + $argLine + ' > ' + (& $q (Join-Path $logs 'os.log')) + ' 2>&1"')
  $a.UseShellExecute = $false; $a.CreateNoWindow = $true; $a.WorkingDirectory = $repo
$a.RedirectStandardOutput = $true; $a.RedirectStandardError = $true
  $a.Environment['BATTALION_HOME'] = Join-Path $DevRoot 'os'
  $a.Environment['HOME'] = Join-Path $DevRoot 'home'; $a.Environment['USERPROFILE'] = Join-Path $DevRoot 'home'
  $a.Environment['APPDATA'] = Join-Path $DevRoot 'roaming'; $a.Environment['LOCALAPPDATA'] = Join-Path $DevRoot 'local'
  [Diagnostics.Process]::Start($a)
}
function Save-Pids($os, $watcher) {
  [pscustomobject]@{ os = (Mark $os); ui = (Mark $vite); watcher = $watcher; uiPort = $uiPort; devRoot = $DevRoot; targetDir = $OsTargetDir } | ConvertTo-Json | Set-Content $pidFile
}
Write-Host ($(if ($built) { 'Building the OS (only what changed; the earlier build is reused)...' } else { "Building the OS for the first time (several minutes). It goes into $OsTargetDir ..." }))
try { Build-Os } catch { & taskkill.exe /PID $vite.Id /T /F 2>&1 | Out-Null; Fail "$_" }
$os = Start-Os
Start-Sleep -Milliseconds 800
if ($os.HasExited) { & taskkill.exe /PID $vite.Id /T /F 2>&1 | Out-Null; Fail "the OS did not start. See $(Join-Path $logs 'os.log')" }
$watcherMark = $(if ($OsWatch) { Mark (Get-Process -Id $PID) } else { $null })
Save-Pids $os $watcherMark
if (-not $Quiet) {
  Write-Host ''
  Write-Host ("Running now: a dev BattalionOS window (pid $($os.Id)) and its screens' dev server (http://127.0.0.1:$uiPort, this PC only).")
  Write-Host "It uses its own data in $DevRoot, never your installed BattalionOS or your tools. The OS is built in $OsTargetDir ($([math]::Round(((Get-Date) - $t0).TotalSeconds)) s this time)."
  Write-Host 'A change to the OS screens (crates/battalion-desktop/ui) shows in under a second.'
  if ($OsWatch) { Write-Host 'A change to the OS Rust code rebuilds and restarts the OS on its own; the time is printed each time. This window stays open to watch (Ctrl+C here ends only the watching).' }
  else { Write-Host 'A change to the OS Rust code needs this command again with -OsWatch (it rebuilds and restarts the OS for you).' }
  Write-Host "To stop everything: scripts/bos-dev.ps1 -Stop -DevRoot `"$DevRoot`""
  Write-Host "Logs: $logs"
}

# 3. A change in the OS's Rust code rebuilds the OS only (the app is not built here) and restarts the OS.
if ($OsWatch) {
  $newest = { (Get-ChildItem (Join-Path $repo 'crates') -Recurse -Include *.rs, Cargo.toml, tauri.conf.json -File | Where-Object { $_.FullName -notmatch '\\(target|node_modules)\\' } | Measure-Object LastWriteTimeUtc -Maximum).Maximum }
  $last = & $newest
  while ($true) {
    Start-Sleep -Milliseconds 700
    $now = & $newest
    if ($now -gt $last) {
      $last = $now; $t = Get-Date
      try { Build-Os } catch { Write-Host "bos-dev: $_" -ForegroundColor Red; continue }
      try { Stop-Ours (Mark $os) | Out-Null; $os = Start-Os } catch { Write-Host "bos-dev: the OS built but could not restart: $_" -ForegroundColor Red; continue }
      Save-Pids $os $watcherMark
      Write-Host ("OS rebuilt and restarted in {0:n1} s, pid {1}" -f ((Get-Date) - $t).TotalSeconds, $os.Id)
    }
  }
}
