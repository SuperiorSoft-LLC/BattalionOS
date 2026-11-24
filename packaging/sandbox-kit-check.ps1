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
        $t = $all | Where-Object { $_.type -eq 'page' -and ("$($_.title) $($_.url)" -match $match) } | Select-Object -First 1
        if (-not $t) { $o.error = 'no page target matches'; return $o }
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
    $res.steps.appInstalled = [bool]$ver -and (Test-Path "$vdir\bin\infantry-daemon.exe")
    $res.steps.icon = [ordered]@{ file = $m.icon; exists = (Test-Path $iconFile); bytes = $(if (Test-Path $iconFile) { (Get-Item $iconFile).Length }); limit = 262144 }
    $res.steps.iconInList = $res.steps.icon.exists -and $res.steps.icon.bytes -le 262144
    Log "app installed: $($res.steps.appInstalled) version $ver; icon in list: $($res.steps.iconInList)"; Save

    # 5. Infantry AI the way its icon does. The icon is a button on the BattalionOS desktop; Enter on it runs the same
    #    open handler as a double-click (start the app, then open its window). Done INSIDE the Sandbox only: the button
    #    is found with UI Automation, focused, and Enter is sent to this Sandbox's own desktop.
    # Sandbox prep, not part of the OS: Windows Sandbox's image has no Visual C++ runtime, and Infantry AI's native sidecar
    # (infantry-sites.exe) needs VCRUNTIME140.dll (without it Windows shows a system error and the app stays on its splash).
    try {
        Invoke-WebRequest -UseBasicParsing 'https://aka.ms/vs/17/release/vc_redist.x64.exe' -OutFile "$env:TEMP\vc_redist.x64.exe" -TimeoutSec 120
        $vp = Start-Process "$env:TEMP\vc_redist.x64.exe" -ArgumentList '/install', '/quiet', '/norestart' -Wait -PassThru
        $res.steps.vcRedistExit = $vp.ExitCode; Log "vc_redist installed in the Sandbox, exit $($vp.ExitCode)"
    } catch { Log "vc_redist step failed: $($_.Exception.Message)" }
    $run = "$osHome\run\infantry-ai"; $ready = "$run\ready.json"
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
    $btn = $null; $names = @(); $tb = Get-Date
    while (-not $btn -and ((Get-Date) - $tb).TotalSeconds -lt 30) {
        $os.Refresh()
        $root = [Windows.Automation.AutomationElement]::FromHandle($os.MainWindowHandle)
        $ct = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Button)
        $btns = $root.FindAll([Windows.Automation.TreeScope]::Descendants, $ct)
        $names = @(for ($i = 0; $i -lt $btns.Count; $i++) { $btns[$i].Current.Name })
        for ($i = 0; $i -lt $btns.Count; $i++) { if ($btns[$i].Current.Name -like '*Infantry AI*') { $btn = $btns[$i]; break } }
        if (-not $btn) { Start-Sleep -Seconds 2 }
    }
    $res.steps.desktopButtons = $names
    if (-not $btn) { throw "the Infantry AI icon was not found on the desktop; buttons: $($names -join ', ')" }
    $btn.SetFocus(); Start-Sleep -Seconds 1
    [Windows.Forms.SendKeys]::SendWait('{ENTER}')
    Log 'icon opened with Enter'; Save
    $t0 = Get-Date
    while (-not (Test-Path $ready) -and ((Get-Date) - $t0).TotalSeconds -lt 90) { Start-Sleep -Milliseconds 500 }
    if (-not (Test-Path $ready)) { throw 'the daemon wrote no ready file after the icon launch' }
    $rf = Get-Content $ready -Raw | ConvertFrom-Json
    $hp = if ($rf.health) { $rf.health } else { '/health' }
    $h = Invoke-WebRequest -UseBasicParsing ($rf.url.TrimEnd('/') + $hp) -TimeoutSec 15
    $res.steps.health = [ordered]@{ url = $rf.url; status = [int]$h.StatusCode; body = $h.Content.Substring(0, [Math]::Min(300, $h.Content.Length)) }
    Log "daemon health $($h.StatusCode) at $($rf.url)"
    Start-Sleep -Seconds 20; WinShot 'infantry-ai-20s' ([W32]::FindWindow([NullString]::Value, 'infantry-ai'))
    Start-Sleep -Seconds 25
    WinShot 'infantry-ai' ([W32]::FindWindow([NullString]::Value, 'infantry-ai'))
    $res.steps.appPage = Cdp '127\.0\.0\.1' @($pageState)
    Log "infantry-ai page: $(($res.steps.appPage.results | ForEach-Object { $_.value }) -join ' ')"; Save
    $res.result = 'PASS'
} catch {
    $res.result = 'FAIL'; $res.error = $_.Exception.Message
    Log "ERROR: $($_.Exception.Message)"; Shot 'error'
}
Save; Log "done: $($res.result)"
