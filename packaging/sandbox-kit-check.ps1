# SPDX-License-Identifier: MPL-2.0
# Runs inside Windows Sandbox only (see make-sandbox-kit.ps1, check\bos-sandbox-check.wsb): copies the kit, installs the
# MSI silently, opens BattalionOS, waits for first-run.state.json and records what it saw in the results folder:
# results.json, log.txt, msi.log and screenshots. It never touches the host: the kit is read-only, results is the
# only writable mapped folder.
param([string]$SandboxDir = 'C:\BattalionOSKit', [string]$Results = 'C:\results', [int]$FirstRunTimeoutSec = 240, [int]$DebugPort = 9222)
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
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class W32 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
}
'@
# A screenshot of one window only (its screen rectangle, brought to the front; no other window is opened).
function WinShot($name, [IntPtr]$h) {
    try {
        if ($h -eq [IntPtr]::Zero) { Log "window screenshot $name skipped: no window"; return }
        [void][W32]::ShowWindow($h, 9); [void][W32]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, 0x3); [void][W32]::SetForegroundWindow($h)
        Start-Sleep -Milliseconds 700
        $r = New-Object W32+RECT; [void][W32]::GetWindowRect($h, [ref]$r)
        $w = $r.R - $r.L; $ht = $r.B - $r.T
        Add-Type -AssemblyName System.Drawing
        $bmp = New-Object Drawing.Bitmap $w, $ht
        $g = [Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object Drawing.Size $w, $ht))
        $bmp.Save("$Results\$name.png", [Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
        Log "window screenshot $name.png (${w}x${ht})"
    } catch { Log "window screenshot $name failed: $($_.Exception.Message)" }
}
# The page's console, errors and state through WebView2 remote debugging (CDP on $DebugPort; the OS is started with
# WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS). One connection: enable, run the expressions, listen for $ListenMs.
function Cdp([string]$match, [string[]]$exprs, [int]$ListenMs = 3000) {
    $o = [ordered]@{ target = $null; events = @(); results = @() }
    try {
        $all = @(Invoke-RestMethod "http://127.0.0.1:$DebugPort/json" -TimeoutSec 5)
        $o.targets = @($all | ForEach-Object { "$($_.type) | $($_.title) | $($_.url)" })
        $t = $all | Where-Object { ($_.type -eq 'page' -or $_.type -eq 'iframe') -and ("$($_.title) $($_.url)" -match $match) } | Select-Object -First 1
        if (-not $t) { $o.error = 'no target matches'; return $o }
        $o.target = "$($t.title) | $($t.url)"
        $ws = New-Object Net.WebSockets.ClientWebSocket
        $ws.ConnectAsync([Uri]$t.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
        $send = { param($id, $m, $p)
            $j = @{ id = $id; method = $m; params = $p } | ConvertTo-Json -Depth 8 -Compress
            $b = [Text.Encoding]::UTF8.GetBytes($j)
            $ws.SendAsync((New-Object ArraySegment[byte] -ArgumentList (,$b)), 'Text', $true, [Threading.CancellationToken]::None).Wait() }
        & $send 1 'Runtime.enable' @{}; & $send 2 'Log.enable' @{}
        $i = 10; foreach ($e in $exprs) { & $send $i 'Runtime.evaluate' @{ expression = $e; returnByValue = $true; awaitPromise = $true }; $i++ }
        $cts = New-Object Threading.CancellationTokenSource $ListenMs
        $buf = New-Object byte[] 65536
        try {
            while ($true) {
                $sb = New-Object Text.StringBuilder
                do { $r = $ws.ReceiveAsync((New-Object ArraySegment[byte] -ArgumentList (,$buf)), $cts.Token).GetAwaiter().GetResult()
                     [void]$sb.Append([Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)) } until ($r.EndOfMessage)
                $m = $sb.ToString() | ConvertFrom-Json
                if ($m.id -ge 10) { $o.results += [pscustomobject]@{ id = $m.id; value = $m.result.result.value; error = $m.result.exceptionDetails.exception.description } }
                elseif ($m.method -eq 'Runtime.consoleAPICalled') { $o.events += "console.$($m.params.type): " + (($m.params.args | ForEach-Object { if ($null -ne $_.value) { $_.value } else { $_.description } }) -join ' ') }
                elseif ($m.method -eq 'Runtime.exceptionThrown') { $d = $m.params.exceptionDetails; $o.events += "EXCEPTION: $($d.text) $($d.exception.description) at $($d.url):$($d.lineNumber)" }
                elseif ($m.method -eq 'Log.entryAdded') { $o.events += "log.$($m.params.entry.level): $($m.params.entry.text) $($m.params.entry.url)" }
            }
        } catch { }   # the listen window ended
        try { $ws.Dispose() } catch { }
    } catch { $o.error = $_.Exception.Message }
    return $o
}
$pageState = "JSON.stringify({ href: location.href, ready: document.readyState, rootChars: (document.getElementById('root')||{innerHTML:''}).innerHTML.length, bodyBg: getComputedStyle(document.body).backgroundColor, tauri: typeof window.__TAURI__, text: document.body.innerText.slice(0, 300), scripts: [].map.call(document.scripts, function (s) { return s.src }) })"
try {
    Log "start; user=$env:USERNAME"
    # 1. The same copy the owner's start script does (without opening Explorer: nothing may cover the BattalionOS window).
    & "$PSScriptRoot\start.ps1" -SandboxDir $SandboxDir -NoOpen
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
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$DebugPort"
    $os = Start-Process $exe.FullName -PassThru; $osStart = Get-Date
    Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
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
    # Only the BattalionOS window, and the page's console and state, at 10, 30 and 60 s after the start.
    $res.steps.page = [ordered]@{}
    foreach ($sec in 10, 30, 60) {
        $wait = $sec - ((Get-Date) - $osStart).TotalSeconds; if ($wait -gt 0) { Start-Sleep -Seconds $wait }
        $os.Refresh(); WinShot "os-${sec}s" $os.MainWindowHandle
        $res.steps.page["s$sec"] = Cdp 'BattalionOS|tauri' @($pageState)
        Log "page at ${sec}s: $(($res.steps.page["s$sec"].results | ForEach-Object { $_.value }) -join ' ')"; Save
    }
    Copy-Item "$Results\os-60s.png" "$Results\desktop.png" -Force -ErrorAction SilentlyContinue
    if (-not $res.steps.firstRunMarker) { throw 'first-run.state.json did not appear in time' }

    # 4. The app is installed and its icon is in what the desktop reads (the installed manifest's icon file, as os_list_apps).
    $res.steps.osHomeTree = @(Get-ChildItem $osHome -Recurse -Depth 3 -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($osHome.Length) } | Select-Object -First 80)
    $appDir = "$osHome\apps\infantry-ai"
    $cur = Get-Content "$appDir\current.json" -Raw | ConvertFrom-Json
    $ver = if ($cur.version) { $cur.version } else { $cur.current }
    $vdir = "$appDir\$ver"
    $m = Get-Content "$vdir\app.json" -Raw | ConvertFrom-Json
    $iconFile = Join-Path $vdir $m.icon
    # An app is a library the OS runs on its own worker (BOS-18): the file to look for is the manifest's own process.<target>.library.
    $lib = $m.process.'windows-x86_64'.library
    $res.steps.appInstalled = [bool]$ver -and [bool]$lib -and (Test-Path (Join-Path $vdir $lib))
    $res.steps.icon = [ordered]@{ file = $m.icon; exists = (Test-Path $iconFile); bytes = $(if (Test-Path $iconFile) { (Get-Item $iconFile).Length }); limit = 262144 }
    $res.steps.iconInList = $res.steps.icon.exists -and $res.steps.icon.bytes -le 262144
    Log "app installed: $($res.steps.appInstalled) version $ver; icon in list: $($res.steps.iconInList)"; Save

    # ---- BOS-19: steps a to g. Everything happens INSIDE this Sandbox (its own desktop, its own input). ----
    function Workers { @(Get-CimInstance Win32_Process -Filter "Name='battalion-desktop.exe'" | Where-Object { $_.CommandLine -match 'bos-app-worker' } | ForEach-Object { $_.ProcessId }) }
    function OsPids { @(Get-CimInstance Win32_Process -Filter "Name='battalion-desktop.exe'" | Where-Object { $_.CommandLine -notmatch 'bos-app-worker' } | ForEach-Object { $_.ProcessId }) }
    function StepLog($step, $text) { Log "STEP ${step}: $text (os pid: $((OsPids) -join ','); app workers: $((Workers) -join ','))" }
    # The OS window is driven through UI Automation (Invoke on the real buttons), inside this Sandbox only. The page's own text is read the same way.
    function UiaAll { $os.Refresh(); $root = [Windows.Automation.AutomationElement]::FromHandle($os.MainWindowHandle); $root.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) }
    function UiaNames { $all = UiaAll; @(for ($i = 0; $i -lt $all.Count; $i++) { $e = $all[$i]; if ($e.Current.Name) { "$($e.Current.ControlType.ProgrammaticName -replace 'ControlType.','')|$($e.Current.Name)" } }) }
    function UiaClick([string]$pattern) {
        $all = UiaAll
        for ($i = 0; $i -lt $all.Count; $i++) { $e = $all[$i]; $n = $e.Current.Name
            if ($n -match $pattern -and $e.Current.ControlType.ProgrammaticName -match 'Button|MenuItem') {
                try { ([Windows.Automation.InvokePattern]$e.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)).Invoke() } catch { $e.SetFocus(); Start-Sleep -Milliseconds 300; [Windows.Forms.SendKeys]::SendWait('{ENTER}') }
                Log "uia click: $n"; return $true } }
        Log "uia click: nothing matches '$pattern'; elements: $((UiaNames | Select-Object -First 40) -join ' ; ')"; return $false
    }
    function FrameText { (UiaNames | Where-Object { $_ -match '^(Text|Document|Edit|Hyperlink)\|' } | Select-Object -First 25) -join ' ; ' }
    function Armory { [void](UiaClick '^Start$'); Start-Sleep 1; [void](UiaClick '^Armory$'); Start-Sleep 3 }
    function Armory2 { }
    $osHomeApps = "$osHome\apps\infantry-ai"
    $startMenu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\BattalionOS Apps"
    $os0 = $os.Id
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
    function DesktopButtons {
        $os.Refresh(); $root = [Windows.Automation.AutomationElement]::FromHandle($os.MainWindowHandle)
        $ct = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button)
        $b = $root.FindAll([Windows.Automation.TreeScope]::Descendants, $ct); @(for ($i = 0; $i -lt $b.Count; $i++) { $b[$i] })
    }
    # What the OS and the app say about themselves (names and sizes; log lines with a token are dropped): for a step that fails.
    function Diag($tag) {
        $d = [ordered]@{ osJson = $(try { (Get-Content "$osHome\os.json" -Raw) -replace '\s+', ' ' } catch { 'none' }); catalogFile = (Test-Path "$SandboxDir\catalog.json"); catalogBytes = $(try { [IO.File]::ReadAllBytes("$SandboxDir\catalog.json").Length } catch { $_.Exception.Message })
            logs = @(Get-ChildItem "$osHome\logs" -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { "$($_.FullName.Substring($osHome.Length)) $($_.Length)" })
            tail = @(Get-ChildItem "$osHome\logs" -Recurse -File -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 3 | ForEach-Object { $n = $_.Name; (Get-Content $_.FullName -Tail 25 -ErrorAction SilentlyContinue | Where-Object { $_ -notmatch '(?i)token|secret|key' } | ForEach-Object { "$n> " + $_.Substring(0, [Math]::Min(220, $_.Length)) }) })
            runFiles = @(Get-ChildItem "$osHome\run" -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName.Substring($osHome.Length) }) }
        $d.probe = Probe; $res.steps["diag_$tag"] = $d; Log "PROBE ${tag}: $(($d.probe.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' | ')"; Log "DIAG ${tag}: os.json=$($d.osJson); catalog readable=$($d.catalogBytes); logs=$($d.logs -join ', ')"; foreach ($l in $d.tail) { Log "  $l" }; Save
    }
    # The page's own way in, probed from here: its page server, and the app's socket with the page's origin (no token is read or printed).
    function Probe {
        $pf = Get-ChildItem "$osHome\run\infantry-ai" -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -match 'port' } | Select-Object -First 1
        $port = if ($pf) { (Get-Content $pf.FullName -Raw).Trim() } else { $null }
        $o = [ordered]@{ portFile = $(if ($pf) { $pf.Name }); port = $port }
        try { $r = Invoke-WebRequest -UseBasicParsing "http://127.0.0.1:$port/?bos-app=infantry-ai" -TimeoutSec 10; $o.page = "$($r.StatusCode) $($r.Content.Length) bytes" } catch { $o.page = "ERR $($_.Exception.Message)" }
        foreach ($origin in @("http://127.0.0.1:$port", 'http://evil.example')) {
            try { $ws = New-Object Net.WebSockets.ClientWebSocket; $ws.Options.SetRequestHeader('Origin', $origin)
                  $ws.ConnectAsync([Uri]'ws://127.0.0.1:18990/ws', [Threading.CancellationToken]::None).Wait(8000) | Out-Null; $o["ws $origin"] = "state $($ws.State)"; $ws.Dispose() }
            catch { $o["ws $origin"] = "ERR $($_.Exception.InnerException.Message)$($_.Exception.Message)" }
        }
        $o
    }
    # a. MSI installed (above), first open installed Infantry AI, icon on the desktop.
    $res.steps.a = [ordered]@{ osPid = $os0; msiExit = $res.steps.msiExit; appInstalled = $res.steps.appInstalled; iconInList = $res.steps.iconInList }
    $tb = Get-Date; $btn = $null
    while (-not $btn -and ((Get-Date) - $tb).TotalSeconds -lt 30) { $btn = DesktopButtons | Where-Object { $_.Current.Name -like '*Infantry AI*' } | Select-Object -First 1; if (-not $btn) { Start-Sleep 2 } }
    $res.steps.a.iconOnDesktop = [bool]$btn
    $res.steps.a.startMenuShortcut = @(Get-ChildItem $startMenu -Filter *.lnk -ErrorAction SilentlyContinue | ForEach-Object { $_.Name })
    WinShot 'a-desktop' ([W32]::FindWindow([NullString]::Value, 'BattalionOS'))
    StepLog 'a' "msi exit $($res.steps.msiExit), installed=$($res.steps.appInstalled), icon on desktop=$([bool]$btn), start menu: $($res.steps.a.startMenuShortcut -join ',')"; Save
    if (-not $btn) { throw 'step a: the Infantry AI icon is not on the desktop' }

    # b. Open it (Enter on the icon = its open handler); page drawn and connected in the window; taskbar; Start menu; minimize and back.
    $run = "$osHome\run\infantry-ai"; $ready = "$run\ready.json"
    $btn.SetFocus(); Start-Sleep 1; [Windows.Forms.SendKeys]::SendWait('{ENTER}')
    $t0 = Get-Date; while (-not (Test-Path $ready) -and ((Get-Date) - $t0).TotalSeconds -lt 120) { Start-Sleep -Milliseconds 500 }
    if (-not (Test-Path $ready)) { throw 'step b: the daemon wrote no ready file' }
    $rf = Get-Content $ready -Raw | ConvertFrom-Json
    $healthUrl = $rf.url.TrimEnd('/') + $(if ($rf.health) { $rf.health } else { '/health' })
    $h = Invoke-WebRequest -UseBasicParsing $healthUrl -TimeoutSec 15
    Start-Sleep 40
    $hwnd = [W32]::FindWindow([NullString]::Value, 'BattalionOS')
    WinShot 'b-app-open' $hwnd
    # the frame's state: address, text, and a marker the check sets so "state kept" can be seen after minimize / restart
    $frameState = "JSON.stringify({ href: location.href, text: document.body.innerText.slice(0, 200), marker: window.__bos19 || null, version: (document.body.innerText.match(/\d+\.\d+\.\d+/)||[null])[0], rootChars: (document.getElementById('root')||{innerHTML:''}).innerHTML.length })"
    $fr = [ordered]@{ text = FrameText; workers = @(Workers); cdpTargets = (Cdp 'BattalionOS|tauri|127' @('1') 1500).error; wv2 = (@(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Select-Object -First 1 | ForEach-Object { $_.CommandLine.Substring(0, [Math]::Min(300, $_.CommandLine.Length)) })) }
    $res.steps.b = [ordered]@{ osPid = $os.Id; daemonHealth = [int]$h.StatusCode; frame = $fr }
    StepLog 'b' "health $($h.StatusCode); frame text: $($fr.text); workers: $($fr.workers -join ','); cdp: $($fr.cdpTargets); webview2: $($fr.wv2)"; Save
    Diag 'b'
    $tbw = [W32]::FindWindow('Shell_TrayWnd', [NullString]::Value); $r = New-Object W32+RECT; [void][W32]::GetWindowRect($tbw, [ref]$r)
    $res.steps.b.windowsTaskbar = [ordered]@{ found = ($tbw -ne [IntPtr]::Zero); rect = "$($r.L),$($r.T),$($r.R),$($r.B)" }
    Shot 'b-taskbar-visible'
    Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class Kb { [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, UIntPtr e); }'
    [Kb]::keybd_event(0x5B, 0, 0, [UIntPtr]::Zero); [Kb]::keybd_event(0x5B, 0, 2, [UIntPtr]::Zero); Start-Sleep 2
    Shot 'b-start-menu-over'
    [Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep 1
    [void][W32]::ShowWindow($hwnd, 6); Start-Sleep 2; Shot 'b-minimized'
    [void][W32]::ShowWindow($hwnd, 9); [void][W32]::SetForegroundWindow($hwnd); Start-Sleep 3
    $fr2 = FrameText
    $res.steps.b.afterRestore = $fr2; $res.steps.b.stateKept = ($fr2 -eq $fr.text) -and ((@(Workers) -join ',') -eq ($fr.workers -join ','))
    WinShot 'b-restored' $hwnd
    StepLog 'b' "minimize and back; state kept (page text and worker unchanged)=$($res.steps.b.stateKept)"; Save

    # c. Process list: no Infantry AI program.
    $procs = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'battalion|infantry|daemon|sites|node|cargo' } |
        ForEach-Object { $cl = "$($_.CommandLine)" -replace '\s+', ' '; [pscustomobject]@{ pid = $_.ProcessId; name = $_.Name; parent = $_.ParentProcessId; cmd = $cl.Substring(0, [Math]::Min(100, $cl.Length)) } })
    $all = @(Get-Process | ForEach-Object { "$($_.ProcessName)#$($_.Id)" })
    $res.steps.c = [ordered]@{ ours = $procs; all = $all; infantryPrograms = @($procs | Where-Object { $_.name -match '^infantry' }) }
    StepLog 'c' "ours: $(($procs | ForEach-Object { "$($_.name)#$($_.pid)<-$($_.parent)" }) -join ', '); infantry-named programs: $(($res.steps.c.infantryPrograms | ForEach-Object { $_.name }) -join ',')"; Shot 'c-processes'; Save

    # d. Close the window, start BattalionOS again: same state, app still running; then start by the shortcut.
    Add-Type -Namespace W -Name M -MemberDefinition '[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);'
    [void][W.M]::PostMessage($hwnd, 0x10, [IntPtr]::Zero, [IntPtr]::Zero); Start-Sleep 3     # WM_CLOSE: the window hides, the OS runs on
    $os.Refresh()
    $health = { try { (Invoke-WebRequest -UseBasicParsing $healthUrl -TimeoutSec 10).StatusCode } catch { 'down' } }
    $res.steps.d = [ordered]@{ osAliveAfterClose = -not $os.HasExited; windowVisibleAfterClose = [W32]::IsWindowVisible($hwnd); healthWhileClosed = (& $health) }
    StepLog 'd' "window closed; OS alive=$($res.steps.d.osAliveAfterClose); window visible=$($res.steps.d.windowVisibleAfterClose); app health $($res.steps.d.healthWhileClosed)"
    Shot 'd-closed'
    Start-Process $exe.FullName; Start-Sleep 8        # a second start: handed to the running one (single instance)
    $hwnd = [W32]::FindWindow([NullString]::Value, 'BattalionOS')
    $fr3 = FrameText
    WinShot 'd-restarted' $hwnd
    $res.steps.d.osPidsAfterSecondStart = @(OsPids); $res.steps.d.sameOsPid = (@(OsPids) -contains $os0) -and (@(OsPids).Count -eq 1)
    $res.steps.d.frameAfter = $fr3; $res.steps.d.stateKept = ($fr3 -eq $fr.text) -and ((@(Workers) -join ',') -eq ($fr.workers -join ','))
    $res.steps.d.appStillRunning = ((& $health) -eq 200)
    StepLog 'd' "second start; same single OS pid=$($res.steps.d.sameOsPid); state kept=$($res.steps.d.stateKept); app running=$($res.steps.d.appStillRunning)"
    [void][W32]::ShowWindow($hwnd, 6); Start-Sleep 2      # minimized first, so the hand-off must bring the window back
    $lnk = Get-ChildItem $startMenu -Filter '*Infantry*.lnk' | Select-Object -First 1
    $sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk.FullName)
    $res.steps.d.shortcut = [ordered]@{ file = $lnk.Name; target = $sc.TargetPath; args = $sc.Arguments; icon = $sc.IconLocation }
    Start-Process $lnk.FullName; Start-Sleep 8
    $hwnd = [W32]::FindWindow([NullString]::Value, 'BattalionOS'); WinShot 'd-shortcut' $hwnd
    $res.steps.d.afterShortcut = [ordered]@{ osPids = @(OsPids); sameOsPid = ((@(OsPids) -contains $os0) -and (@(OsPids).Count -eq 1)); frame = (FrameText); workers = @(Workers) }
    StepLog 'd' "shortcut $($lnk.Name) -> $($sc.TargetPath) $($sc.Arguments); same OS pid=$($res.steps.d.afterShortcut.sameOsPid)"; Save

    # e. Update from the Armory to the second version: OS pid unchanged, page shows the new version.
    $verBefore = (Get-Content "$osHomeApps\current.json" -Raw | ConvertFrom-Json).version
    $res.steps.e = [ordered]@{ osPidBefore = $os0; versionBefore = $verBefore }
    if (Test-Path "$SandboxDir\feed-v2.json") {
        Copy-Item "$SandboxDir\feed-v2.json" "$SandboxDir\feed.json" -Force      # the catalog now offers the second version
        Armory; Shot 'e-armory'; Diag 'e'
        # the Armory reads the feed again on Reload; the page then offers 'Update to <version>'
        [void](UiaClick '^Reload$'); Start-Sleep 6
        $res.steps.e.updateResult = UiaClick '^Update'
        Start-Sleep 40
        $res.steps.e.versionAfter = (Get-Content "$osHomeApps\current.json" -Raw | ConvertFrom-Json).version
        $res.steps.e.osPidAfter = @(OsPids); $res.steps.e.sameOsPid = ((@(OsPids) -contains $os0) -and (@(OsPids).Count -eq 1))
        $hwnd = [W32]::FindWindow([NullString]::Value, 'BattalionOS'); WinShot 'e-updated' $hwnd
        $res.steps.e.frame = FrameText
        StepLog 'e' "update $verBefore -> $($res.steps.e.versionAfter) (clicked=$($res.steps.e.updateResult)); same OS pid=$($res.steps.e.sameOsPid); frame: $($res.steps.e.frame)"
    } else { StepLog 'e' 'SKIPPED: no feed-v2.json in the kit' }
    Save

    # f. Remove from the Armory: icon and shortcut gone, its data folder kept; install again: data is back.
    $dataDir = "$osHome\data\infantry-ai"; New-Item -ItemType Directory -Force $dataDir | Out-Null
    Set-Content "$dataDir\bos19-sentinel.txt" 'bos19 data survives remove'
    Armory; [void](UiaClick 'Open Infantry AI page'); Start-Sleep 2; [void](UiaClick '^Remove'); Start-Sleep 1; $rem = UiaClick '^Remove$'; Start-Sleep 10
    $hwnd = [W32]::FindWindow([NullString]::Value, 'BattalionOS'); WinShot 'f-removed' $hwnd
    $iconGone = -not (DesktopButtons | Where-Object { $_.Current.Name -like '*Infantry AI*' })
    $res.steps.f = [ordered]@{ removeResult = $rem; iconGone = $iconGone; shortcutGone = -not (Get-ChildItem $startMenu -Filter '*Infantry*.lnk' -ErrorAction SilentlyContinue); dataKept = (Test-Path "$dataDir\bos19-sentinel.txt"); osPid = @(OsPids) }
    StepLog 'f' "removed: icon gone=$iconGone shortcut gone=$($res.steps.f.shortcutGone) data kept=$($res.steps.f.dataKept)"
    Armory; [void](UiaClick 'Open Infantry AI page'); Start-Sleep 2; $ins = UiaClick '^Install$'; Start-Sleep 30
    $res.steps.f.installResult = $ins
    $res.steps.f.dataBack = (Test-Path "$dataDir\bos19-sentinel.txt")
    $res.steps.f.shortcutBack = [bool](Get-ChildItem $startMenu -Filter '*Infantry*.lnk' -ErrorAction SilentlyContinue)
    $hwnd = [W32]::FindWindow([NullString]::Value, 'BattalionOS'); WinShot 'f-reinstalled' $hwnd
    StepLog 'f' "installed again: data back=$($res.steps.f.dataBack) shortcut back=$($res.steps.f.shortcutBack)"; Save

    # g. Exit: no process of ours left.
    [void](UiaClick '^Start$'); Start-Sleep 1; [void](UiaClick '^Exit BattalionOS$'); Start-Sleep 15
    $left = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'battalion|infantry' } | ForEach-Object { "$($_.Name)#$($_.ProcessId)" })
    $res.steps.g = [ordered]@{ left = $left }
    Shot 'g-exited'; StepLog 'g' "exit: processes left: $($left -join ',')"; Save
    $res.result = if ($left.Count -eq 0) { 'PASS' } else { 'FAIL' }
} catch {
    $res.result = 'FAIL'; $res.error = $_.Exception.Message
    Log "ERROR: $($_.Exception.Message)"; Shot 'error'
}
Save; Log "done: $($res.result)"
