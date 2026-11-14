<#
SSHeesh measuring kit for Windows.

Run it INSIDE SSHeesh's own terminal ("This computer", not a server):

    powershell -ExecutionPolicy Bypass -File .\measure.ps1          every hands-off measurement, about 4 minutes
    powershell -ExecutionPolicy Bypass -File .\measure.ps1 flood    a long flood: press Ctrl+C once, count the seconds
    powershell -ExecutionPolicy Bypass -File .\measure.ps1 idle     only the 60 s idle measurement
    powershell -ExecutionPolicy Bypass -File .\measure.ps1 notify   a test notification after 8 s
    powershell -ExecutionPolicy Bypass -File .\measure.ps1 clean    delete the test files this kit made

Single parts: info, memory, cat, cyrillic.

It measures the app whose terminal it runs in (0.10.x or the 1.0 prototype) and writes a short
text file with numbers only: no computer name, user name, address or path.

Options: -AppPid N (the app's process, if it is not found), -Seconds N (idle length),
         -Mb N (flood size), -Runs N.

Written for the Windows PowerShell 5.1 that every Windows has. This file is plain ASCII on
purpose, so it reads the same whatever the system's language is.
#>
param(
    [Parameter(Position = 0)][string]$Part = 'all',
    [int]$AppPid = 0,
    [int]$Seconds = 60,
    [int]$Mb = 40,
    [int]$Runs = 3
)

$ErrorActionPreference = 'Stop'
$KitVersion = 1
$Inv = [Globalization.CultureInfo]::InvariantCulture
$Utf8 = New-Object System.Text.UTF8Encoding($false)
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Work = Join-Path ([IO.Path]::GetTempPath()) 'ssheesh-kit'
$Esc = [string][char]27
$script:Result = $null
$script:AppPid = $AppPid
$script:AppName = ''
$script:AppKind = 'current'

# ---------------------------------------------------------------- small tools

function Fmt([double]$x, [int]$digits) {
    return $x.ToString('F' + $digits, $Inv)
}

function Say([string]$line) {
    Write-Host $line
    if ($script:Result) {
        [IO.File]::AppendAllText($script:Result, $line + "`r`n", $Utf8)
    }
}

function Note([string]$line) {
    Write-Host $line
}

function Total($rows, [string]$prop) {
    $t = 0.0
    foreach ($r in $rows) { $t += [double]($r.$prop) }
    return $t
}

function Median($values) {
    $v = @($values | Sort-Object)
    if ($v.Count -eq 0) { return $null }
    if ($v.Count % 2 -eq 1) { return [double]$v[($v.Count - 1) / 2] }
    return ([double]$v[$v.Count / 2 - 1] + [double]$v[$v.Count / 2]) / 2
}

# ---------------------------------------------------------------- which app, which processes

# Every process: its parent, its name and (for the web view only) whose web view it is.
function Get-ProcessTable {
    $map = @{}
    foreach ($p in Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, CommandLine) {
        $map[[int]$p.ProcessId] = @{
            Parent = [int]$p.ParentProcessId
            Name   = [string]$p.Name
            Ours   = ([string]$p.CommandLine) -match '--webview-exe-name=\S*ssheesh'
        }
    }
    return $map
}

function Find-App($map) {
    $p = $PID
    for ($i = 0; $i -lt 30 -and $map.ContainsKey($p); $i++) {
        if ($map[$p].Name -match 'ssheesh') { return $p }
        $p = $map[$p].Parent
    }
    return 0
}

# The program itself and, for 0.10.x, its web view's processes. Shells and ssh are never counted.
function Get-AppPids {
    $ids = New-Object System.Collections.ArrayList
    [void]$ids.Add($script:AppPid)
    if ($script:AppKind -eq 'current') {
        $map = Get-ProcessTable
        foreach ($id in @($map.Keys)) {
            if ($map[$id].Name -ne 'msedgewebview2.exe') { continue }
            $mine = $map[$id].Ours
            if (-not $mine) {
                $q = $id
                for ($i = 0; $i -lt 30 -and $map.ContainsKey($q) -and $q -ne $script:AppPid; $i++) { $q = $map[$q].Parent }
                $mine = ($q -eq $script:AppPid)
            }
            if ($mine) { [void]$ids.Add($id) }
        }
    }
    return , $ids.ToArray()
}

# One row per process: Id, Name, Cpu (seconds), PrivateMb (memory that is its own), WorkingMb.
function Get-Snapshot {
    $rows = New-Object System.Collections.ArrayList
    foreach ($id in (Get-AppPids)) {
        try {
            $p = Get-Process -Id $id -ErrorAction Stop
            $row = New-Object psobject -Property @{
                Id        = $id
                Name      = $p.ProcessName
                Cpu       = $p.TotalProcessorTime.TotalSeconds
                PrivateMb = $p.PrivateMemorySize64 / 1MB
                WorkingMb = $p.WorkingSet64 / 1MB
            }
            [void]$rows.Add($row)
        } catch {
            # The process ended, or Windows will not tell us about it.
        }
    }
    return , $rows.ToArray()
}

function Get-ProcessSummary($rows) {
    $parts = @()
    foreach ($g in ($rows | Group-Object Name | Sort-Object Name)) { $parts += ('{0} x{1}' -f $g.Name, $g.Count) }
    return ($parts -join ', ')
}

# ---------------------------------------------------------------- test files (made-up text only)

function Write-Repeated([string]$file, [string]$block, [int64]$want) {
    $bytes = $Utf8.GetBytes($block)
    $n = [int][Math]::Ceiling($want / $bytes.Length)
    $fs = [IO.File]::Create($file)
    try {
        for ($k = 0; $k -lt $n; $k++) { $fs.Write($bytes, 0, $bytes.Length) }
    } finally {
        $fs.Close()
    }
}

function New-FloodFile {
    $file = Join-Path $Work ('stream-{0}.txt' -f $Mb)
    $want = [int64]$Mb * 1MB
    if ((Test-Path $file) -and ((Get-Item $file).Length -ge $want)) { return $file }
    Note "Making a $Mb MB test file (once)..."
    $jp1 = -join ([char[]](0x65E5, 0x672C, 0x8A9E, 0x306E, 0x30ED, 0x30B0, 0x884C))
    $jp2 = -join ([char[]](0x5B8C, 0x4E86, 0x3057, 0x307E, 0x3057, 0x305F))
    $jp3 = -join ([char[]](0x8868, 0x793A, 0x5E45, 0x306E, 0x30C6, 0x30B9, 0x30C8))
    $rocket = [char]::ConvertFromUtf32(0x1F680)
    $check = [string][char]0x2705
    $sb = New-Object System.Text.StringBuilder
    for ($i = 0; $i -lt 7000; $i++) {
        $mm = [int]([Math]::Floor($i / 60) % 60)
        $ss = [int]($i % 60)
        $stamp = '{0}[90m2026-11-14 10:{1:D2}:{2:D2}{0}[0m ' -f $Esc, $mm, $ss
        switch ($i % 10) {
            3 { $rest = '{0}[33mWARN{0}[0m  web-7731 queue is {1} items deep, worker {2} is slow {0}[35m[retry {3}]{0}[0m' -f $Esc, ($i * 7 % 900), ($i % 8), ($i % 3) }
            6 { $rest = '{0}[32mINFO{0}[0m  db-7732 {1} {2:D5} {3} {4} {5} {6}' -f $Esc, $jp1, $i, $jp2, $rocket, $check, $jp3 }
            9 { $rest = '{0}[1;31mERROR{0}[0m web-7731 upstream 203.0.113.{1} timed out after {2} ms {0}[4mGET /api/items/{3}{0}[0m' -f $Esc, ($i % 250 + 1), ($i * 13 % 5000), $i }
            default { $rest = '{0}[32mINFO{0}[0m  web-7731 request {1:D6} GET /api/items/{2} 200 in {3} ms {0}[36m{4} bytes{0}[0m' -f $Esc, $i, ($i * 31 % 9973), ($i * 17 % 400), ($i * 113 % 65536) }
        }
        [void]$sb.Append($stamp).Append($rest).Append("`n")
    }
    Write-Repeated $file $sb.ToString() $want
    return $file
}

# Two files with the same number of characters: plain Latin letters, and Russian letters.
function New-LetterFiles {
    $latin = Join-Path $Work 'latin-20.txt'
    $cyr = Join-Path $Work 'cyrillic-20.txt'
    if ((Test-Path $latin) -and (Test-Path $cyr)) { return @($latin, $cyr) }
    Note 'Making two letter files (once)...'
    $l = 'The quick brown fox jumps over the lazy dog and runs back to the quiet river before the night falls.'
    # A hundred Russian letters and spaces, built from their numbers so this file stays ASCII.
    $sbc = New-Object System.Text.StringBuilder
    for ($i = 0; $i -lt 100; $i++) {
        if ($i % 7 -eq 6) { [void]$sbc.Append(' ') } else { [void]$sbc.Append([char](0x0430 + ($i * 5 % 32))) }
    }
    $c = $sbc.ToString()
    foreach ($pair in @(@($latin, $l), @($cyr, $c))) {
        $sb = New-Object System.Text.StringBuilder
        for ($i = 0; $i -lt 400; $i++) { [void]$sb.Append($pair[1]).Append("`n") }
        $block = $sb.ToString()
        Write-Repeated $pair[0] $block ([int64]$Utf8.GetByteCount($block) * 500)
    }
    return @($latin, $cyr)
}

# Prints a file straight to this console with cmd's "type" and returns the seconds it took.
# The program is started directly so that nothing stands between it and the terminal.
function Invoke-Type([string]$file) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
    $psi.Arguments = '/d /c type ' + [IO.Path]::GetFileName($file)
    $psi.WorkingDirectory = [IO.Path]::GetDirectoryName($file)
    $psi.UseShellExecute = $false
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = [Diagnostics.Process]::Start($psi)
    $p.WaitForExit()
    $sw.Stop()
    $p.Dispose()
    return $sw.Elapsed.TotalSeconds
}

# Runs "type" several times; the console reads the files as UTF-8 meanwhile.
function Measure-Type([string]$file, [string]$label) {
    $times = @()
    $old = [Console]::OutputEncoding
    try {
        [Console]::OutputEncoding = $Utf8
        for ($i = 1; $i -le $Runs; $i++) {
            $t = Invoke-Type $file
            Write-Host ($Esc + '[0m')
            Say ('{0} run {1}: type returned after {2} s' -f $label, $i, (Fmt $t 3))
            $times += $t
            Start-Sleep -Seconds 2
        }
    } finally {
        [Console]::OutputEncoding = $old
    }
    return (Median $times)
}

# ---------------------------------------------------------------- the parts

function Open-Result {
    $dir = Join-Path $Here 'results'
    try {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    } catch {
        $dir = $Work
    }
    $stamp = (Get-Date).ToString('yyyyMMdd-HHmmss', $Inv)
    $script:Result = Join-Path $dir ('ssheesh-kit-windows-{0}-{1}.txt' -f $script:AppKind, $stamp)
    Say "SSHeesh measuring kit $KitVersion, part: $Part"
    Say 'numbers only: no computer name, user name, address or path is written here'
}

function Part-Info {
    Say '--- info'
    Say ('date: ' + (Get-Date).ToString('yyyy-MM-dd HH:mm', $Inv))
    $os = Get-CimInstance Win32_OperatingSystem
    Say ('system: {0} (build {1})' -f ([string]$os.Caption).Trim(), $os.BuildNumber)
    foreach ($c in @(Get-CimInstance Win32_Processor)) {
        Say ('cpu: {0}, {1} threads' -f ([string]$c.Name).Trim(), $c.NumberOfLogicalProcessors)
    }
    $cs = Get-CimInstance Win32_ComputerSystem
    Say ('memory_gb: ' + (Fmt ([double]$cs.TotalPhysicalMemory / 1GB) 1))
    foreach ($v in @(Get-CimInstance Win32_VideoController)) {
        Say ('display: {0}, {1}x{2} at {3} Hz' -f $v.Name, $v.CurrentHorizontalResolution, $v.CurrentVerticalResolution, $v.CurrentRefreshRate)
    }
    $battery = @(Get-CimInstance Win32_Battery)
    if ($battery.Count -eq 0) {
        Say 'power: no battery'
    } elseif ($battery[0].BatteryStatus -eq 2) {
        Say 'power: on mains'
    } else {
        Say 'power: on battery'
    }
    $version = 'version unknown'
    try {
        $v = (Get-Process -Id $script:AppPid).MainModule.FileVersionInfo.ProductVersion
        if ($v) { $version = $v }
    } catch {
    }
    Say ('app: {0} {1} ({2})' -f $script:AppName, $version, $script:AppKind)
    $size = $Host.UI.RawUI.WindowSize
    Say ('terminal: {0} columns x {1} rows' -f $size.Width, $size.Height)
    Say ('app processes: ' + (Get-ProcessSummary (Get-Snapshot)))
}

function Part-Memory {
    Say '--- memory (10,100 lines of 120 characters)'
    $a = Get-Snapshot
    $sb = New-Object System.Text.StringBuilder
    for ($i = 1; $i -le 10100; $i++) {
        [void]$sb.Append($i.ToString($Inv).PadLeft(120, '0')).Append("`r`n")
    }
    [Console]::Out.Write($sb.ToString())
    [Console]::Out.Flush()
    Start-Sleep -Seconds 5
    $b = Get-Snapshot
    $m0 = Total $a 'PrivateMb'
    $m1 = Total $b 'PrivateMb'
    Say 'memory_method: private bytes'
    Say ('memory_before_mb: {0} (working set {1})' -f (Fmt $m0 1), (Fmt (Total $a 'WorkingMb') 1))
    Say ('memory_after_mb: {0} (working set {1})' -f (Fmt $m1 1), (Fmt (Total $b 'WorkingMb') 1))
    Say ('memory_growth_mb: ' + (Fmt ($m1 - $m0) 1))
    foreach ($r in $b) {
        Say ('  process {0}: {1} MB (working set {2})' -f $r.Name, (Fmt $r.PrivateMb 1), (Fmt $r.WorkingMb 1))
    }
}

function Part-Cat {
    $file = New-FloodFile
    $size = (Get-Item $file).Length
    Say ('--- cat of a {0} MB file' -f (Fmt ($size / 1MB) 1))
    $c0 = Total (Get-Snapshot) 'Cpu'
    $t = Measure-Type $file 'cat'
    $c1 = Total (Get-Snapshot) 'Cpu'
    Say ('cat_shell_s: ' + (Fmt $t 3))
    Say ('cat_mb_per_s: ' + (Fmt ($size / 1MB / $t) 1))
    Say ('cat_50_mb_would_take_s: ' + (Fmt ($t * 52428800 / $size) 3))
    Say ('cat_app_cpu_s_per_run: ' + (Fmt (($c1 - $c0) / $Runs) 3))
    Say ('memory_after_cat_mb: ' + (Fmt (Total (Get-Snapshot) 'PrivateMb') 1))
}

function Part-Cyrillic {
    $files = New-LetterFiles
    Say '--- Latin against Russian letters (20,200,000 characters each)'
    $latin = Measure-Type $files[0] 'latin'
    Say ('latin_shell_s: ' + (Fmt $latin 3))
    $cyr = Measure-Type $files[1] 'cyrillic'
    Say ('cyrillic_shell_s: ' + (Fmt $cyr 3))
    Say ('cyrillic_slower_by: ' + (Fmt ($cyr / $latin) 2) + ' times')
}

function Part-Idle {
    Say "--- idle ($Seconds s)"
    Note ''
    Note ">>> Do not touch the keyboard or the mouse for $($Seconds + 10) seconds."
    Note '>>> Leave this window in front, with the cursor blinking. I will beep when done.'
    Start-Sleep -Seconds 8
    $a = Get-Snapshot
    $sw = [Diagnostics.Stopwatch]::StartNew()
    Start-Sleep -Seconds $Seconds
    $b = Get-Snapshot
    $secs = $sw.Elapsed.TotalSeconds
    try { [Console]::Beep() } catch { }
    $before = @{}
    foreach ($r in $a) { $before[$r.Id] = $r.Cpu }
    $sum = 0.0
    $lines = @()
    foreach ($r in $b) {
        if (-not $before.ContainsKey($r.Id)) { continue }
        $d = ($r.Cpu - $before[$r.Id]) / $secs * 100
        $sum += $d
        $lines += ('  process {0}: {1}%' -f $r.Name, (Fmt $d 2))
    }
    Say ('idle_seconds: ' + (Fmt $secs 3))
    Say ('idle_cpu_percent_of_one_core: ' + (Fmt $sum 2))
    foreach ($l in $lines) { Say $l }
    Say ('app processes: ' + @($b).Count)
}

function Part-Flood {
    $file = New-FloodFile
    Note ''
    Note '>>> Text will fly for a while. Press Ctrl+C ONCE when it does,'
    Note '>>> and count the seconds until the prompt is back.'
    Start-Sleep -Seconds 4
    $old = [Console]::OutputEncoding
    try {
        [Console]::OutputEncoding = $Utf8
        for ($i = 0; $i -lt 4; $i++) { [void](Invoke-Type $file) }
    } finally {
        [Console]::OutputEncoding = $old
    }
}

function Part-Notify {
    Note 'In 8 seconds this terminal asks for a notification.'
    Note 'Switch to another program NOW, then click the notification if one appears.'
    Start-Sleep -Seconds 8
    [Console]::Out.Write($Esc + ']9;SSHeesh kit test' + [char]7)
    [Console]::Out.Flush()
    Note 'Sent.'
}

function Close-Result {
    if (-not $script:Result) { return }
    Note ''
    try {
        Set-Clipboard -Value ([IO.File]::ReadAllText($script:Result, $Utf8))
        Note 'The result is in the clipboard: paste it into the chat.'
    } catch {
    }
    Note ('The result is also saved in: ' + $script:Result)
}

# ---------------------------------------------------------------- start

$known = @('all', 'info', 'memory', 'cat', 'cyrillic', 'idle', 'flood', 'notify', 'clean')
if ($known -notcontains $Part) {
    Write-Host "unknown part $Part. Parts: $($known -join ', ')"
    exit 2
}

try {
    if ($Part -eq 'clean') {
        if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
        Write-Host "Deleted $Work"
        exit 0
    }
    if ($Part -eq 'notify') {
        Part-Notify
        exit 0
    }

    New-Item -ItemType Directory -Force -Path $Work | Out-Null
    $table = Get-ProcessTable
    if ($script:AppPid -eq 0) { $script:AppPid = Find-App $table }
    if ($script:AppPid -eq 0 -or -not $table.ContainsKey($script:AppPid)) {
        Write-Host 'I am not running inside SSHeesh.'
        Write-Host 'Open a terminal for "This computer" in SSHeesh (not a server) and run me there.'
        exit 1
    }
    $script:AppName = $table[$script:AppPid].Name -replace '\.exe$', ''
    if ($script:AppName -match 'proto') { $script:AppKind = 'prototype' }

    Open-Result
    switch ($Part) {
        'all' {
            Part-Info
            Part-Memory
            Part-Cyrillic
            Part-Cat
            Start-Sleep -Seconds 5
            Part-Idle
        }
        'info' { Part-Info }
        'memory' { Part-Info; Part-Memory }
        'cat' { Part-Info; Part-Cat }
        'cyrillic' { Part-Info; Part-Cyrillic }
        'idle' { Part-Info; Part-Idle }
        'flood' { Part-Info; Part-Flood }
    }
    Close-Result
} catch {
    Write-Host ''
    Write-Host 'The kit stopped on an error. Please copy the lines below into the chat:'
    Write-Host ('  ' + $_.Exception.Message)
    Write-Host ('  at line ' + $_.InvocationInfo.ScriptLineNumber + ': ' + ([string]$_.InvocationInfo.Line).Trim())
    exit 1
}
