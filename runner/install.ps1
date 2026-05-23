# Pi Dash runner installer (Windows / PowerShell).
#
# PowerShell mirror of install.sh. Wraps the cargo-dist-generated
# `pidash-installer.ps1`, then launches `pidash auth login` so the user
# lands in the device-code flow on the same install one-liner. The
# runner is a "set up once, forget" daemon driven by Pi Dash cloud, so
# the natural moment to authenticate is right now while the user is
# at the terminal — not the first time they happen to type `pidash`
# themselves (which may be never).
#
# Usage:
#   irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.ps1 | iex

$ErrorActionPreference = 'Stop'

$installerUrl = 'https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.ps1'

Write-Host '==> Downloading pidash...'
Invoke-Expression (Invoke-RestMethod $installerUrl)

# cargo-dist drops pidash.exe into $env:USERPROFILE\.local\bin (the
# install-path = "$HOME/.local/bin" in dist-workspace.toml resolves
# to USERPROFILE on Windows). If a future release moves it, surface
# a clear error instead of silently continuing.
$pidashBin = Join-Path $env:USERPROFILE '.local\bin\pidash.exe'
if (-not (Test-Path -PathType Leaf $pidashBin)) {
    Write-Host ''
    Write-Host "pidash.exe not found at $pidashBin after install."
    Write-Host "Run ``$installerUrl`` manually, then ``pidash auth login``."
    exit 1
}

Write-Host ''
Write-Host '==> Starting authentication...'
Write-Host ''

# Headless detection: in non-interactive contexts (Windows scheduled
# tasks, Packer/Ansible provisioners, MSI ExecuteSequence, CI on a
# Windows runner without a desktop session) there's no one to approve
# the device code. Skip the auto-launch and point at the headless
# enrollment path instead.
#
# Unix's /dev/tty reattach trick has no Windows equivalent and isn't
# needed: PowerShell child processes inherit the console host directly,
# so even when our own stdin is the `irm | iex` pipe, `pidash auth
# login` still reads keystrokes from the user's console.
if (-not [Environment]::UserInteractive) {
    Write-Host 'No interactive console detected — skipping auto-auth.'
    Write-Host 'Run `pidash auth login` from a terminal, or use the headless'
    Write-Host 'enrollment path: `pidash connect --url <URL> --token <TOKEN>`.'
    exit 0
}

& $pidashBin auth login
exit $LASTEXITCODE
