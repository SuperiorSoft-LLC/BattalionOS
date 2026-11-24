# SPDX-License-Identifier: MPL-2.0
# Runs inside Windows Sandbox only (see make-sandbox-kit.ps1, check\bos-sandbox-check.wsb): copies the kit, installs the
# MSI silently, opens BattalionOS, waits for first-run.state.json and records what it saw in the results folder:
# results.json, log.txt, msi.log and screenshots. It never touches the host: the kit is read-only, results is the
# only writable mapped folder.
param([string]$SandboxDir = 'C:\BattalionOSKit', [string]$Results = 'C:\results', [int]$FirstRunTimeoutSec = 240)
if ($env:USERNAME -ne 'WDAGUtilityAccount') { Write-Error 'refused: this is for Windows Sandbox only'; exit 2 }
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Results | Out-Null
$log = "$Results\log.txt"
function Log($m) { "[{0:HH:mm:ss}] {1}" -f (Get-Date), $m | Add-Content $log }
$res = [ordered]@{ steps = [ordered]@{} }
function Save() { $res | ConvertTo-Json -Depth 8 | Set-Content "$Results\results.json" -Encoding utf8 }
function Shot($name) {
    try {
        Add-Type -AssemblyName System.Windows.Forms, System.Drawing
        $b = [Windows.Forms.SystemInformation]::VirtualScreen
        $bmp = New-Object Drawing.Bitmap $b.Width, $b.Height
        $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location, [Drawing.Point]::Empty, $b.Size)
        $bmp.Save("$Results\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
        Log "screenshot $name.png"
    } catch { Log "screenshot $name failed: $($_.Exception.Message)" }
}
try {
    Log "start; user=$env:USERNAME"
    # 1. The same copy the owner's start script does.
    & "$PSScriptRoot\start.ps1" -SandboxDir $SandboxDir
    $res.steps.kitCopied = (Test-Path "$SandboxDir\BattalionOS.msi") -and (Test-Path "$SandboxDir\catalog.json") -and (Test-Path "$SandboxDir\feed.json")
    Log "kit copied: $($res.steps.kitCopied)"; Save

    # 2. MSI, silent, per user (box defaults to checked).
    $p = Start-Process msiexec -ArgumentList "/i `"$SandboxDir\BattalionOS.msi`" /qn /l*v `"$Results\msi.log`"" -Wait -PassThru
    $res.steps.msiExit = $p.ExitCode; Log "msiexec exit $($p.ExitCode)"; Save
    if ($p.ExitCode -ne 0) { throw "MSI install failed with exit code $($p.ExitCode) (see msi.log)" }
    $exe = Get-ChildItem "$env:LOCALAPPDATA\Programs", $env:ProgramFiles -Recurse -Filter battalion-desktop.exe -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $exe) { throw 'battalion-desktop.exe not found after install' }
    $res.steps.exe = $exe.FullName
    $plan = Join-Path $exe.DirectoryName 'first-run.json'
    $res.steps.planFile = if (Test-Path $plan) { Get-Content $plan -Raw | ConvertFrom-Json } else { $null }
    Log "installed at $($exe.DirectoryName); plan file present: $([bool]$res.steps.planFile)"; Save

    # 3. Open BattalionOS and wait for the done marker.
    $osHome = "$env:LOCALAPPDATA\SuperiorSoft\BattalionOS"
    $os = Start-Process $exe.FullName -PassThru
    $res.steps.osPid = $os.Id; Log "BattalionOS started pid $($os.Id)"
    $t0 = Get-Date; $marker = "$osHome\first-run.state.json"
    while (-not (Test-Path $marker) -and ((Get-Date) - $t0).TotalSeconds -lt $FirstRunTimeoutSec) {
        if ($os.HasExited) { throw "BattalionOS exited early with code $($os.ExitCode)" }
        Start-Sleep -Seconds 2
    }
    $res.steps.firstRunSeconds = [int]((Get-Date) - $t0).TotalSeconds
    $res.steps.firstRunMarker = Test-Path $marker
    Log "first-run marker: $($res.steps.firstRunMarker) after $($res.steps.firstRunSeconds)s"
    if ($res.steps.firstRunMarker) { $res.steps.firstRunState = Get-Content $marker -Raw | ConvertFrom-Json }
    Start-Sleep -Seconds 3; Shot 'desktop'; Save
    if (-not $res.steps.firstRunMarker) { throw 'first-run.state.json did not appear in time' }

    # 4. The app is installed and its icon is in what the desktop reads (the installed manifest's icon file, as os_list_apps).
    $res.steps.osHomeTree = @(Get-ChildItem $osHome -Recurse -Depth 3 -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($osHome.Length) } | Select-Object -First 80)
    $appDir = "$osHome\apps\infantry-ai"
    $cur = Get-Content "$appDir\current.json" -Raw | ConvertFrom-Json
    $ver = if ($cur.version) { $cur.version } else { $cur.current }
    $vdir = "$appDir\$ver"
    $m = Get-Content "$vdir\app.json" -Raw | ConvertFrom-Json
    $iconFile = Join-Path $vdir $m.icon
    $res.steps.appInstalled = [bool]$ver -and (Test-Path "$vdir\bin\infantry-daemon.exe")
    $res.steps.icon = [ordered]@{ file = $m.icon; exists = (Test-Path $iconFile); bytes = $(if (Test-Path $iconFile) { (Get-Item $iconFile).Length }); limit = 262144 }
    $res.steps.iconInList = $res.steps.icon.exists -and $res.steps.icon.bytes -le 262144
    Log "app installed: $($res.steps.appInstalled) version $ver; icon in list: $($res.steps.iconInList)"; Save

    # 5. The daemon answers /health. Started the way the OS starts an app (contract v1 environment, see
    #    battalion-host supervisor.rs), because a click on the icon cannot be made from a script.
    $run = "$osHome\run\infantry-ai"; New-Item -ItemType Directory -Force $run | Out-Null
    $state = "$env:LOCALAPPDATA\SuperiorSoft\InfantryAI-Rust"; New-Item -ItemType Directory -Force $state | Out-Null
    $ready = "$run\ready.json"; Remove-Item $ready -ErrorAction SilentlyContinue
    $env:BATTALION_CONTRACT = '1'; $env:BATTALION_APP_ID = 'infantry-ai'; $env:BATTALION_APP_VERSION = "$ver"
    $env:BATTALION_APP_DIR = $vdir; $env:BATTALION_STATE_DIR = $state; $env:BATTALION_READY_FILE = $ready
    $env:BATTALION_TOKEN_FILE = "$run\token"; Set-Content "$run\token" ([guid]::NewGuid().ToString('N'))
    $d = Start-Process "$vdir\bin\infantry-daemon.exe" -PassThru -WindowStyle Hidden -RedirectStandardOutput "$Results\daemon.out.txt" -RedirectStandardError "$Results\daemon.err.txt"
    $t0 = Get-Date
    while (-not (Test-Path $ready) -and ((Get-Date) - $t0).TotalSeconds -lt 90 -and -not $d.HasExited) { Start-Sleep -Milliseconds 500 }
    if (-not (Test-Path $ready)) { throw "daemon wrote no ready file (exited: $($d.HasExited))" }
    $rf = Get-Content $ready -Raw | ConvertFrom-Json
    $hp = if ($rf.health) { $rf.health } else { '/health' }
    $h = Invoke-WebRequest -UseBasicParsing ($rf.url.TrimEnd('/') + $hp) -TimeoutSec 15
    $res.steps.health = [ordered]@{ url = $rf.url; status = [int]$h.StatusCode; body = $h.Content.Substring(0, [Math]::Min(300, $h.Content.Length)) }
    Log "daemon health $($h.StatusCode) at $($rf.url)"
    Shot 'after-daemon'
    Stop-Process -Id $d.Id -Force -ErrorAction SilentlyContinue
    $res.result = 'PASS'
} catch {
    $res.result = 'FAIL'; $res.error = $_.Exception.Message
    Log "ERROR: $($_.Exception.Message)"; Shot 'error'
}
Save; Log "done: $($res.result)"
