# SPDX-License-Identifier: MPL-2.0
# Runs inside Windows Sandbox at logon (see make-sandbox-kit.ps1). It only copies the read-only kit to the folder the
# catalog urls name and opens that folder. YOU run the MSI from there (double-click BattalionOS.msi).
param([string]$SandboxDir = 'C:\BattalionOSKit')
$kit = Split-Path -Parent $MyInvocation.MyCommand.Path
if ($env:USERNAME -ne 'WDAGUtilityAccount') { Write-Error 'refused: this is for Windows Sandbox only'; exit 2 }
New-Item -ItemType Directory -Force $SandboxDir | Out-Null
Copy-Item "$kit\*" $SandboxDir -Recurse -Force
Start-Process explorer.exe $SandboxDir
