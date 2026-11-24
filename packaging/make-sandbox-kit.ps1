# SPDX-License-Identifier: MPL-2.0
# Generates the first-install kit for Windows Sandbox: a folder with a real app package, a LOCAL catalog and feed
# (file: urls, a throwaway public key), an MSI whose first-run.json points at that catalog, and one .wsb to double-click.
# Nothing is published, signed with a real key or installed on this machine.
#
# Usage (Windows, from the BattalionOS checkout):
#   pwsh packaging/make-sandbox-kit.ps1 -Package <dir> -Kit <dir> [-Results <dir>] [-SkipMsiBuild]
#
#   -Package   output of the app packager, made with --base-url file:///C:/BattalionOSKit/ (the SandboxDir below):
#              <app>.bosapp, .bosapp.sig, feed.json, catalog-entry.json. Any throwaway key file in it is NOT copied.
#   -PackageV2 a second package of the same app (same key, newer version, same --base-url): its .bosapp and .sig go into the kit and
#              its feed as feed-v2.json; the check copies it over feed.json to update from the Armory (BOS-19).
#   -Kit       the folder to fill; the .wsb maps it READ-ONLY into the Sandbox.
#   -Results   a second folder, writable, for the check run (only results go there). Default: <Kit>-results.
#   -SandboxDir  where the Sandbox copies the kit; the catalog/feed urls name it. Default C:\BattalionOSKit.
#   -SkipMsiBuild  use the MSI already in <Kit> (built by an earlier run) instead of building one.
#
# The MSI is built with the committed packaging/first-run.json swapped for the local one for the length of the
# build only (restored in a finally block; the committed file stays https). No code is changed.
#
# Kit layout:
#   bos-sandbox.wsb        double-click: clean Sandbox, kit copied to SandboxDir, folder opened; you run the MSI
#   check\bos-sandbox-check.wsb   the same, then installs silently and writes results to the Results folder
#   BattalionOS.msi, catalog.json, feed.json, <app>.bosapp(.sig), start.ps1, check.ps1
param(
    [Parameter(Mandatory)][string]$Package,
    [Parameter(Mandatory)][string]$Kit,
    [string]$Results,
    [string]$PackageV2,
    [string]$SandboxDir = 'C:\BattalionOSKit',
    [switch]$SkipMsiBuild
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Package = (Resolve-Path $Package).Path
New-Item -ItemType Directory -Force $Kit | Out-Null
$Kit = (Resolve-Path $Kit).Path
if (-not $Results) { $Results = "$Kit-results" }
New-Item -ItemType Directory -Force $Results | Out-Null
$Results = (Resolve-Path $Results).Path
if ($Results.StartsWith($Kit.TrimEnd('\') + '\') -or $Kit.StartsWith($Results.TrimEnd('\') + '\')) { throw 'Kit and Results must be separate folders' }
$urlBase = ([Uri]($SandboxDir.TrimEnd('\') + '\')).AbsoluteUri   # file:///C:/BattalionOSKit/

# 1. The package, its signature and the feed (never the key file).
$pkg = Get-ChildItem $Package -Filter *.bosapp | Select-Object -First 1
if (-not $pkg) { throw "no .bosapp in $Package" }
$feed = Get-Content "$Package\feed.json" -Raw | ConvertFrom-Json
foreach ($p in $feed.platforms.PSObject.Properties) {
    if (-not $p.Value.url.StartsWith($urlBase)) { throw "feed url '$($p.Value.url)' does not start with $urlBase; repack with --base-url $urlBase" }
}
Copy-Item $pkg.FullName, "$($pkg.FullName).sig", "$Package\feed.json" $Kit -Force
if ($PackageV2) {
    $PackageV2 = (Resolve-Path $PackageV2).Path
    $pkg2 = Get-ChildItem $PackageV2 -Filter *.bosapp | Select-Object -First 1
    if (-not $pkg2) { throw "no .bosapp in $PackageV2" }
    $feed2 = Get-Content "$PackageV2\feed.json" -Raw | ConvertFrom-Json
    if ([version]$feed2.version -le [version]$feed.version) { throw "the second package ($($feed2.version)) must be newer than the first ($($feed.version))" }
    foreach ($p in $feed2.platforms.PSObject.Properties) { if (-not $p.Value.url.StartsWith($urlBase)) { throw "feed-v2 url '$($p.Value.url)' does not start with $urlBase" } }
    Copy-Item $pkg2.FullName, "$($pkg2.FullName).sig" $Kit -Force
    Copy-Item "$PackageV2\feed.json" "$Kit\feed-v2.json" -Force
}

# 2. The local catalog (schema 2): the packaged entry plus its icon as an inline image.
$entry = Get-Content "$Package\catalog-entry.json" -Raw | ConvertFrom-Json
if (-not $entry.channels.stable.StartsWith($urlBase)) { throw "catalog entry channel url must start with $urlBase" }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($pkg.FullName)
try {
    $manifest = $null
    $m = $zip.GetEntry('app.json'); $r = New-Object IO.StreamReader($m.Open()); $manifest = $r.ReadToEnd() | ConvertFrom-Json; $r.Dispose()
    $ie = $zip.GetEntry($manifest.icon); $ms = New-Object IO.MemoryStream; $s = $ie.Open(); $s.CopyTo($ms); $s.Dispose()
    $icon = 'data:image/png;base64,' + [Convert]::ToBase64String($ms.ToArray())
} finally { $zip.Dispose() }
$entry | Add-Member -NotePropertyName icon -NotePropertyValue $icon -Force
$catalog = [ordered]@{ schema = 2; apps = @($entry) }
$catalog | ConvertTo-Json -Depth 10 | Set-Content "$Kit\catalog.json" -Encoding utf8
$appId = $manifest.id

# 3. The MSI with a local first-run.json (built with the committed file swapped, then restored).
if (-not $SkipMsiBuild) {
    $plan = Join-Path $repo 'packaging\first-run.json'
    $saved = Get-Content $plan -Raw
    try {
        $local = [ordered]@{ sources = @(($urlBase + 'catalog.json')); apps = @(@{ id = $appId; channel = 'stable' }) }
        $local | ConvertTo-Json -Depth 5 | Set-Content $plan -Encoding utf8
        Push-Location (Join-Path $repo 'crates\battalion-desktop')
        try {
            pnpm --dir ui install --frozen-lockfile; if ($LASTEXITCODE) { throw 'ui install failed' }
            pnpm --dir ui build; if ($LASTEXITCODE) { throw 'ui build failed' }
            pnpm dlx '@tauri-apps/cli@2' build --config ../../packaging/tauri.msi.conf.json --bundles msi
            if ($LASTEXITCODE) { throw 'MSI build failed' }
        } finally { Pop-Location }
    } finally {
        [IO.File]::WriteAllText($plan, $saved)   # byte-for-byte what was committed
    }
    $meta = cargo metadata --format-version 1 --no-deps --manifest-path (Join-Path $repo 'Cargo.toml') | ConvertFrom-Json
    $msi = Get-ChildItem (Join-Path $meta.target_directory 'release\bundle\msi') -Filter *.msi | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $msi) { throw 'no MSI was built' }
    Copy-Item $msi.FullName "$Kit\BattalionOS.msi" -Force
}
if (-not (Test-Path "$Kit\BattalionOS.msi")) { throw 'no BattalionOS.msi in the kit' }

# 4. The two small scripts and the .wsb files.
Copy-Item (Join-Path $PSScriptRoot 'sandbox-kit-start.ps1') "$Kit\start.ps1" -Force
Copy-Item (Join-Path $PSScriptRoot 'sandbox-kit-check.ps1') "$Kit\check.ps1" -Force
$in = 'C:\kit'
function Wsb($hostKit, $extraFolder, $logon) {
    $x = [Security.SecurityElement]
    $folders = "<MappedFolder><HostFolder>$($x::Escape($hostKit))</HostFolder><SandboxFolder>$in</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>"
    if ($extraFolder) { $folders += "<MappedFolder><HostFolder>$($x::Escape($extraFolder))</HostFolder><SandboxFolder>C:\results</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>" }
    @"
<Configuration>
  <Networking>Enable</Networking>
  <VGpu>Default</VGpu>
  <ClipboardRedirection>Disable</ClipboardRedirection>
  <PrinterRedirection>Disable</PrinterRedirection>
  <AudioInput>Disable</AudioInput>
  <VideoInput>Disable</VideoInput>
  <MappedFolders>$folders</MappedFolders>
  <LogonCommand><Command>$($x::Escape($logon))</Command></LogonCommand>
</Configuration>
"@
}
$ps = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File'
Wsb $Kit $null "$ps $in\start.ps1 -SandboxDir $SandboxDir" | Set-Content "$Kit\bos-sandbox.wsb" -Encoding utf8
New-Item -ItemType Directory -Force "$Kit\check" | Out-Null
Wsb $Kit $Results "$ps $in\check.ps1 -SandboxDir $SandboxDir -Results C:\results" | Set-Content "$Kit\check\bos-sandbox-check.wsb" -Encoding utf8
"kit ready: $Kit"
Get-ChildItem $Kit -Recurse -File | ForEach-Object { '{0,12:N0}  {1}' -f $_.Length, $_.FullName.Substring($Kit.Length + 1) }
