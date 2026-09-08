# Terminal fallback using the official Windows installers. Does not copy credentials.
param(
    [ValidateSet('claude', 'antigravity', 'both')][string]$Provider = 'both',
    [switch]$Yes
)
$ErrorActionPreference = 'Stop'
if (-not $Yes) {
    $HopperAnswer = Read-Host "Install missing $Provider CLIs using official installers? [y/N]"
    if ($HopperAnswer -notin @('y', 'Y')) { exit 0 }
}
$env:PATH = "$env:USERPROFILE\.local\bin;$env:LOCALAPPDATA\agy\bin;$env:PATH"
function Install-HopperProvider([string]$Binary, [string]$Url, [string[]]$InstallerArgs = @()) {
    if (Get-Command $Binary -ErrorAction SilentlyContinue) { Write-Host "$Binary is already installed."; return }
    $HopperInstaller = Join-Path ([IO.Path]::GetTempPath()) ("hopper-" + [guid]::NewGuid().ToString() + '.ps1')
    try {
        Invoke-WebRequest -Uri $Url -OutFile $HopperInstaller -UseBasicParsing
        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $HopperInstaller @InstallerArgs
        if ($LASTEXITCODE -ne 0) { throw "$Binary installer failed." }
        if (-not (Get-Command $Binary -ErrorAction SilentlyContinue)) { throw "$Binary was not found. Restart your terminal and check the provider's instructions." }
    } finally { Remove-Item $HopperInstaller -ErrorAction SilentlyContinue }
}
if ($Provider -in @('claude', 'both')) {
    Install-HopperProvider 'claude' 'https://claude.ai/install.ps1'
    Write-Host 'Sign in using: claude auth login'
}
if ($Provider -in @('antigravity', 'both')) {
    Install-HopperProvider 'agy' 'https://antigravity.google/cli/install.ps1' @('--skip-aliases')
    Write-Host 'Sign in using: agy'
}
Write-Host 'Open Hopper > Settings > Codex > Connect your coding agents, then test each connection.'
