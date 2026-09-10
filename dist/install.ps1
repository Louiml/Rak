# rak-setup bootstrap — downloads and runs the rak-setup wizard for Windows.
#
# Usage (PowerShell):
#   iwr -useb https://raw.githubusercontent.com/Louiml/Rak/main/dist/install.ps1 | iex
#
# Pass extra args via $args, e.g.:
#   & ([scriptblock]::Create((iwr -useb <url>).Content)) --yes --install rakc,rakpkg

$ErrorActionPreference = 'Stop'

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64') {
    Write-Error "rak-setup: unsupported arch $arch (only x86_64 for now)"
    exit 1
}

$asset = 'rak-setup-windows-x86_64.exe'
$url = "https://github.com/Louiml/Rak/releases/latest/download/$asset"

$tmp = [System.IO.Path]::GetTempFileName()
$tmp = [System.IO.Path]::ChangeExtension($tmp, '.exe')
Write-Host "[rak-setup] downloading $url"
Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing

& $tmp @args
Remove-Item $tmp -ErrorAction SilentlyContinue
