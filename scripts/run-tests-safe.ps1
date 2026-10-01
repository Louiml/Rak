# Run a test binary with a memory watchdog.
#
# Why this exists: a stack overflow in the test binary is turned into a Windows
# Error Reporting event, and with `CrashDumpEnabled = 3` (automatic memory dump,
# the system default) that writes a multi-gigabyte MEMORY.DMP. On one machine
# that turned a single failing test into a process holding 7.5 GB and 45 minutes
# of CPU, which is enough to make the box unusable. Changing the dump setting
# needs administrator rights, so the fix has to happen here instead: kill the
# process before it can grow.
#
# Limits come from the environment rather than flags, because PowerShell's
# `ValueFromRemainingArguments` swallows anything that looks like a named
# parameter and passes it to the child instead.
#
#   RAK_TEST_MEM_MB    kill above this RSS in MB      (default 3000)
#   RAK_TEST_TIMEOUT   kill after this many seconds   (default 900)
#
# Usage:
#   scripts\run-tests-safe.ps1 <test-binary> [test args...]

param(
    [Parameter(Mandatory = $true, Position = 0)][string]$Binary,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$TestArgs
)

$ErrorActionPreference = "Stop"

$MemoryMb = if ($env:RAK_TEST_MEM_MB) { [int]$env:RAK_TEST_MEM_MB } else { 3000 }
$TimeoutSec = if ($env:RAK_TEST_TIMEOUT) { [int]$env:RAK_TEST_TIMEOUT } else { 900 }

if (-not (Test-Path -LiteralPath $Binary)) {
    Write-Error "no such test binary: $Binary"
    exit 2
}
if (-not $TestArgs) { $TestArgs = @() }

$outFile = [System.IO.Path]::GetTempFileName()
$errFile = [System.IO.Path]::GetTempFileName()

# `Start-Process -ArgumentList` rejects an empty collection, so build the
# invocation differently depending on whether there are test args at all.
$startArgs = @{
    FilePath               = $Binary
    RedirectStandardOutput = $outFile
    RedirectStandardError  = $errFile
    NoNewWindow            = $true
    PassThru               = $true
}
if ($TestArgs.Count -gt 0) { $startArgs["ArgumentList"] = $TestArgs }
$proc = Start-Process @startArgs

Write-Host "watching pid $($proc.Id), limit ${MemoryMb} MB / $TimeoutSec s"
$peak = 0
$mb = 0
$killed = $null
$deadline = (Get-Date).AddSeconds($TimeoutSec)

while (-not $proc.HasExited) {
    Start-Sleep -Milliseconds 500
    try {
        $proc.Refresh()
        $mb = [math]::Round($proc.WorkingSet64 / 1MB, 0)
        if ($mb -gt $peak) { $peak = $mb }
    } catch {
        break   # process gone
    }

    if ($mb -gt $MemoryMb) {
        $killed = "memory: ${mb} MB exceeded the ${MemoryMb} MB limit"
        break
    }
    if ((Get-Date) -gt $deadline) {
        $killed = "timeout after $TimeoutSec s"
        break
    }
}

if ($killed) {
    Write-Host "ABORTED ($killed) - peak was ${peak} MB, killing pid $($proc.Id)"
    # Kill the whole tree. A Rak test that has spawned helper threads or a child
    # `rakc` must not leave anything running behind.
    & taskkill /F /T /PID $proc.Id 2>&1 | Out-Null
    Start-Sleep -Seconds 2
    Get-Process -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like "rakc*" } |
        ForEach-Object {
            Write-Host "  cleaning leftover pid $($_.Id)"
            Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
        }
    Get-Content $outFile -ErrorAction SilentlyContinue
    Get-Content $errFile -ErrorAction SilentlyContinue
    Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
    Write-Host "ABORTED: $killed"
    exit 3
}

$code = $proc.ExitCode
Get-Content $outFile -ErrorAction SilentlyContinue
Get-Content $errFile -ErrorAction SilentlyContinue
Remove-Item $outFile, $errFile -Force -ErrorAction SilentlyContinue
Write-Host ("peak RSS {0} MB, exit {1}" -f $peak, $code)
exit $code
