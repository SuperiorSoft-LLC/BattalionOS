# SPDX-License-Identifier: MPL-2.0
# Installs the BattalionOS MSI silently into a scratch folder, once with the Infantry AI box checked and once
# unchecked, and checks the plan file, the dialog, the default and the uninstall. Per user, no admin.
# Usage: pwsh packaging/test-msi.ps1 [-Msi path]   (build it first: packaging/README.md)
param([string]$Msi = (Join-Path $PSScriptRoot '..\target\release\bundle\msi\BattalionOS_0.1.0_x64_en-US.msi'))
$ErrorActionPreference = 'Stop'
$Msi = (Resolve-Path $Msi).Path
$fail = 0
function Check($name, $ok) { if ($ok) { "PASS $name" } else { "FAIL $name"; $script:fail++ } }

$db = (New-Object -ComObject WindowsInstaller.Installer).OpenDatabase($Msi, 0)
function Q($sql) { $v = $db.OpenView($sql); $v.Execute(); $r = $v.Fetch(); if ($r) { $r.StringData(1) } }
Check 'apps page is in the MSI' ((Q "SELECT Dialog FROM Dialog WHERE Dialog='InstallAppsDlg'") -eq 'InstallAppsDlg')
Check 'box is checked by default' ((Q "SELECT Value FROM Property WHERE Property='INSTALL_INFANTRY'") -eq '1')
Check 'plan file is a conditional component' ((Q "SELECT Component FROM Component WHERE Component='FirstRunPlan'") -eq 'FirstRunPlan')

$home_ = Join-Path $env:TEMP "bos-msi-home-$PID"
New-Item -ItemType Directory -Force "$home_\data\infantry-ai" | Out-Null
Set-Content "$home_\data\infantry-ai\keep.txt" 'app state'
foreach ($choice in 1, 0) {
    $dir = Join-Path $env:TEMP "bos-msi-$PID-$choice"
    $p = Start-Process msiexec -ArgumentList "/i `"$Msi`" /qn INSTALL_INFANTRY=$choice INSTALLDIR=`"$dir`"" -Wait -PassThru
    Check "install INSTALL_INFANTRY=$choice exits 0" ($p.ExitCode -eq 0)
    Check "INSTALL_INFANTRY=$choice installs the OS" (Test-Path "$dir\battalion-desktop.exe")
    Check "INSTALL_INFANTRY=$choice plan file present = $($choice -eq 1)" ((Test-Path "$dir\first-run.json") -eq ($choice -eq 1))
    if ($choice -eq 1) {
        $plan = Get-Content "$dir\first-run.json" -Raw | ConvertFrom-Json
        Check 'plan names infantry-ai on stable' ($plan.apps[0].id -eq 'infantry-ai' -and $plan.apps[0].channel -eq 'stable')
    }
    $p = Start-Process msiexec -ArgumentList "/x `"$Msi`" /qn" -Wait -PassThru
    Check "uninstall (choice $choice) exits 0 and removes the folder" ($p.ExitCode -eq 0 -and -not (Test-Path $dir))
}
Check 'uninstall leaves an app state folder alone' (Test-Path "$home_\data\infantry-ai\keep.txt")
Remove-Item $home_ -Recurse -Force
if ($fail) { "$fail check(s) failed"; exit 1 } else { 'all passed' }
