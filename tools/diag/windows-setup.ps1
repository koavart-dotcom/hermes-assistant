# Register the JEA endpoint "HermesDiag" for Hermes read-only diagnostics (docs/DIAG.md).
# Run as administrator on each Windows host (or deploy the same through GPO/DSC):
#
#   .\windows-setup.ps1 -Principal 'DEKA\HERMES-SRV$'        # the server computer account
#   .\windows-setup.ps1 -Principal 'DEKA\svc-hermes'         # or the service's domain account
#
# The session runs as a temporary virtual account, but only the cmdlets of
# HermesDiag.psrc are visible (NoLanguage mode): reading WMI classes for the OS,
# disks and services, event logs and processes. Nothing can be changed.
param(
    [Parameter(Mandatory = $true)][string]$Principal
)
$ErrorActionPreference = 'Stop'

$module = Join-Path $env:ProgramFiles 'WindowsPowerShell\Modules\HermesDiag'
New-Item -ItemType Directory -Force -Path (Join-Path $module 'RoleCapabilities') | Out-Null
Copy-Item (Join-Path $PSScriptRoot 'HermesDiag.psrc') (Join-Path $module 'RoleCapabilities\HermesDiag.psrc') -Force
New-ModuleManifest -Path (Join-Path $module 'HermesDiag.psd1') -ModuleVersion '1.0' -Description 'Hermes JEA role'

$pssc = Join-Path $env:TEMP 'HermesDiag.pssc'
New-PSSessionConfigurationFile -Path $pssc -SessionType RestrictedRemoteServer `
    -RunAsVirtualAccount -TranscriptDirectory 'C:\ProgramData\JEA\Transcripts\HermesDiag' `
    -RoleDefinitions @{ $Principal = @{ RoleCapabilities = 'HermesDiag' } }

if (Get-PSSessionConfiguration -Name HermesDiag -ErrorAction SilentlyContinue) {
    Unregister-PSSessionConfiguration -Name HermesDiag -Force
}
Register-PSSessionConfiguration -Name HermesDiag -Path $pssc -Force | Out-Null
Remove-Item $pssc
"ok: JEA endpoint HermesDiag for $Principal; transcripts in C:\ProgramData\JEA\Transcripts\HermesDiag"
