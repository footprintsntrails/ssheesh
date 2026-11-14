<#
Starts the SSHeesh 1.0 terminal prototype with its own meter switched on (Windows).

    powershell -ExecutionPolicy Bypass -File .\run-prototype.ps1 [-Program path\to\ssheesh-proto.exe] [-Hud]

Without -Program it looks for ssheesh-proto.exe beside this folder. The meter writes numbers
only (times, frame counts, memory) into results\proto-meter-<time>.jsonl; never text you typed,
server names or paths. -Hud also shows the numbers in the title row; leave it off for the idle
measurement, because the row redraws once a second.
#>
param(
    [string]$Program = '',
    [switch]$Hud
)

$ErrorActionPreference = 'Stop'
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Program) {
    foreach ($c in @((Join-Path $Here 'ssheesh-proto.exe'), (Join-Path (Split-Path -Parent $Here) 'ssheesh-proto.exe'))) {
        if (Test-Path $c) { $Program = $c; break }
    }
}
if (-not $Program -or -not (Test-Path $Program)) {
    Write-Host 'I cannot find ssheesh-proto.exe. Give me its path:'
    Write-Host '    powershell -ExecutionPolicy Bypass -File .\run-prototype.ps1 -Program C:\path\to\ssheesh-proto.exe'
    exit 1
}
$dir = Join-Path $Here 'results'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$out = Join-Path $dir ('proto-meter-' + (Get-Date).ToString('yyyyMMdd-HHmmss') + '.jsonl')
Write-Host "Meter file: $out"
Write-Host "Close the prototype's window when you are done; then send that file."
$arguments = @('--local', '--metrics', $out)
if ($Hud) { $arguments += '--hud' }
& $Program @arguments
