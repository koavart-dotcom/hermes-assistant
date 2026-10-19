# Register the JEA endpoint "HermesDiag" for Hermes read-only diagnostics (docs/DIAG.md).
# Run as administrator on each Windows host (or deploy the same through GPO/DSC):
#
#   .\windows-setup.ps1 -Principal 'DEKA\HERMES-SRV$'        # the server computer account
#   .\windows-setup.ps1 -Principal 'DEKA\svc-hermes'         # or the service's domain account
#   .\windows-setup.ps1 -Principal 'DEKA\HERMES-SRV$' -FixServices W3SVC,Spooler
#
# -FixServices also registers the endpoint "HermesFix" for the web panel fixes
# (crates/hermes-core/src/fix.rs): only Restart-Service / Start-Service, and only for
# the listed services (ValidateSet). Without -FixServices "HermesFix" is removed.
#
# The session runs as a temporary virtual account, but only the cmdlets of
# HermesDiag.psrc are visible (NoLanguage mode): reading WMI classes for the OS,
# disks and services, event logs and processes. Nothing can be changed.
param(
    [Parameter(Mandatory = $true)][string]$Principal,
    [string[]]$FixServices = @()
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

if (Get-PSSessionConfiguration -Name HermesFix -ErrorAction SilentlyContinue) {
    Unregister-PSSessionConfiguration -Name HermesFix -Force
}
if ($FixServices.Count -gt 0) {
    foreach ($s in $FixServices) {
        if ($s -notmatch '^[A-Za-z0-9@._-]+$') { throw "bad service name: $s" }
    }
    $fixModule = Join-Path $env:ProgramFiles 'WindowsPowerShell\Modules\HermesFix'
    New-Item -ItemType Directory -Force -Path (Join-Path $fixModule 'RoleCapabilities') | Out-Null
    New-PSRoleCapabilityFile -Path (Join-Path $fixModule 'RoleCapabilities\HermesFix.psrc') `
        -Description 'Hermes: restart/start of the listed services only' `
        -VisibleCmdlets @(
            @{ Name = 'Restart-Service'; Parameters = @{ Name = 'Name'; ValidateSet = $FixServices }, @{ Name = 'PassThru' } },
            @{ Name = 'Start-Service'; Parameters = @{ Name = 'Name'; ValidateSet = $FixServices }, @{ Name = 'PassThru' } },
            'Select-Object'
        )
    New-ModuleManifest -Path (Join-Path $fixModule 'HermesFix.psd1') -ModuleVersion '1.0' -Description 'Hermes JEA fix role'
    $fixPssc = Join-Path $env:TEMP 'HermesFix.pssc'
    New-PSSessionConfigurationFile -Path $fixPssc -SessionType RestrictedRemoteServer `
        -RunAsVirtualAccount -TranscriptDirectory 'C:\ProgramData\JEA\Transcripts\HermesFix' `
        -RoleDefinitions @{ $Principal = @{ RoleCapabilities = 'HermesFix' } }
    Register-PSSessionConfiguration -Name HermesFix -Path $fixPssc -Force | Out-Null
    Remove-Item $fixPssc
    "ok: JEA endpoint HermesFix (restart/start: $($FixServices -join ', '))"
}
