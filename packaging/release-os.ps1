# SPDX-License-Identifier: MPL-2.0
# BOS-30: builds the BattalionOS MSI, signs it, and writes latest.json (the feed the OS reads to update itself).
# Windows, from the BattalionOS checkout. Publishing is not done here: the three files in -OutDir are what a release carries.
#
# The signing key and its password are read when this runs and are never printed or written anywhere:
#   $env:BATTALION_UPDATER_KEY_FILE       path of the minisign private key (the Tauri signer's key file)
#   $env:BATTALION_UPDATER_KEY_PASSWORD   its password (leave unset for a key without one)
# Or pass -KeyFile (a path). The password is only ever read from the environment, never from a parameter or the command line.
#
# Real release (key on this machine, outside every repo):
#   $env:BATTALION_UPDATER_KEY_FILE = '<path to the private key>'
#   $env:BATTALION_UPDATER_KEY_PASSWORD = '<its password>'        # from a prompt or a password manager, not from a script in a repo
#   pwsh packaging/release-os.ps1 -Version 0.2.0 -OutDir C:\release-0.2.0 -Notes 'what changed'
#   -> BattalionOS_0.2.0_x64_en-US.msi, its .sig, latest.json. Upload the three to the release; latest.json is read from
#      https://github.com/SuperiorSoft-LLC/BattalionOS/releases/latest/download/latest.json
#
# Test build (Sandbox proof only; never a release): -TestBuild adds the cargo feature `test-update`, which lets the build read
# BATTALION_OS_UPDATE_FEED / BATTALION_OS_UPDATE_PUBKEY, and -BaseUrl names where the MSI will be served (a file: url for a local
# feed). Use the THROWAWAY key with it. -FirstRun swaps packaging/first-run.json for the length of the build only (restored in a
# finally block), so the installed OS installs the apps from a local catalog.
param(
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [Parameter(Mandatory)][string]$OutDir,
    [string]$Notes = '',
    [string]$KeyFile,
    [string]$BaseUrl,
    [switch]$TestBuild,
    [string]$FirstRun,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $KeyFile) { $KeyFile = $env:BATTALION_UPDATER_KEY_FILE }
if (-not $KeyFile) { throw 'no signing key: set BATTALION_UPDATER_KEY_FILE (a path) or pass -KeyFile' }
if (-not (Test-Path -LiteralPath $KeyFile -PathType Leaf)) { throw "the signing key file does not exist: $KeyFile" }
if ($KeyFile.StartsWith($repo, [StringComparison]::OrdinalIgnoreCase)) { throw 'the signing key must live outside the repository' }
if (-not $BaseUrl) { $BaseUrl = "https://github.com/SuperiorSoft-LLC/BattalionOS/releases/download/v$Version/" }
if (-not $BaseUrl.EndsWith('/')) { $BaseUrl += '/' }
if ($TestBuild -eq $false -and $BaseUrl.StartsWith('file:')) { throw 'a file: BaseUrl is for a -TestBuild only' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

# The key goes to the Tauri signer through its environment variables (never a command line). The password stays in the env.
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = $KeyFile
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = if ($env:BATTALION_UPDATER_KEY_PASSWORD) { $env:BATTALION_UPDATER_KEY_PASSWORD } else { '' }
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue

$msiName = "BattalionOS_${Version}_x64_en-US.msi"
$plan = Join-Path $repo 'packaging\first-run.json'
$saved = $null
$cargoToml = Join-Path $repo 'crates\battalion-desktop\Cargo.toml'
$cargoSaved = [IO.File]::ReadAllBytes($cargoToml)   # the Tauri CLI rewrites it (adds feature lists); put it back as it was
try {
    if (-not $SkipBuild) {
        # The version is data of the build: a config laid over the committed one, for this build only.
        $msiConf = Get-Content (Join-Path $repo 'packaging\tauri.msi.conf.json') -Raw | ConvertFrom-Json
        $msiConf | Add-Member -NotePropertyName version -NotePropertyValue $Version -Force
        $msiConf.bundle | Add-Member -NotePropertyName createUpdaterArtifacts -NotePropertyValue $false -Force
        $conf = Join-Path $OutDir 'build-config.json'
        # Relative paths in the committed config are relative to crates\battalion-desktop; keep them so by writing it into packaging\.
        $conf = Join-Path $repo "packaging\.build-config-$Version.json"
        $msiConf | ConvertTo-Json -Depth 10 | Set-Content $conf -Encoding utf8
        if ($FirstRun) { $saved = Get-Content $plan -Raw; Copy-Item -LiteralPath $FirstRun $plan -Force }
        Push-Location (Join-Path $repo 'crates\battalion-desktop')
        try {
            pnpm --dir ui install --frozen-lockfile; if ($LASTEXITCODE) { throw 'ui install failed' }
            pnpm --dir ui build; if ($LASTEXITCODE) { throw 'ui build failed' }
            $tauriArgs = @('dlx', '@tauri-apps/cli@2', 'build', '--config', "../../packaging/.build-config-$Version.json", '--bundles', 'msi')
            if ($TestBuild) { $tauriArgs += @('--features', 'test-update') }
            pnpm @tauriArgs
            if ($LASTEXITCODE) { throw 'MSI build failed' }
        } finally { Pop-Location; Remove-Item $conf -ErrorAction SilentlyContinue }
    }
    $meta = cargo metadata --format-version 1 --no-deps --manifest-path (Join-Path $repo 'Cargo.toml') | ConvertFrom-Json
    $built = Join-Path $meta.target_directory "release\bundle\msi\$msiName"
    if (-not (Test-Path $built)) { throw "the build did not make $msiName under $($meta.target_directory)\release\bundle\msi" }
    Copy-Item $built (Join-Path $OutDir $msiName) -Force
} finally {
    [IO.File]::WriteAllBytes($cargoToml, $cargoSaved)
    if ($null -ne $saved) { [IO.File]::WriteAllText($plan, $saved) }   # byte-for-byte what was committed
}

# Sign. The Tauri signer writes <msi>.sig: base64 of the minisign signature file, the form latest.json carries.
$msi = Join-Path $OutDir $msiName
Remove-Item "$msi.sig" -ErrorAction SilentlyContinue
Push-Location (Join-Path $repo 'crates\battalion-desktop')
try {
    pnpm dlx '@tauri-apps/cli@2' signer sign $msi | Out-Null
    if ($LASTEXITCODE) { throw 'signing failed (wrong key file or password?)' }
} finally { Pop-Location }
if (-not (Test-Path "$msi.sig")) { throw 'the signer wrote no signature' }
$sig = (Get-Content "$msi.sig" -Raw).Trim()

$latest = [ordered]@{
    version  = $Version
    notes    = $Notes
    pub_date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{ 'windows-x86_64' = [ordered]@{ signature = $sig; url = $BaseUrl + $msiName } }
}
$latest | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $OutDir 'latest.json') -Encoding utf8
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PATH, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
"release files in ${OutDir}:"
Get-ChildItem $OutDir -File | ForEach-Object { '{0,12:N0}  {1}' -f $_.Length, $_.Name }
