# Smoke test for cargo-dist's generated PowerShell installer in the current
# PowerShell host. CI invokes this once with Windows PowerShell 5.1 and once
# with PowerShell 7.
#
# Run cargo-dist with --artifacts=lies first. That produces the real installer
# template and harmless local stub archives, letting this test exercise the
# default install path without downloading or running a real pidash binary.

param(
    [Parameter(Mandatory = $true)]
    [string] $GeneratedInstaller
)

$ErrorActionPreference = 'Stop'
$generatedInstaller = (Resolve-Path $GeneratedInstaller).Path
$artifactDir = Split-Path -Parent $generatedInstaller
$artifactUrl = ([uri]($artifactDir + [System.IO.Path]::DirectorySeparatorChar)).AbsoluteUri
$originalUserProfile = $env:USERPROFILE
$originalHomeDrive = $env:HOMEDRIVE
$originalHomePath = $env:HOMEPATH
$originalLocalAppData = $env:LOCALAPPDATA
$originalEnvHome = $env:HOME
$originalPowerShellHome = $HOME
$work = Join-Path ([System.IO.Path]::GetTempPath()) "pidash-generated-installer-test-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $work | Out-Null

try {
    $home_ = Join-Path $work 'home'
    $localAppData = Join-Path $work 'local-app-data'
    New-Item -ItemType Directory -Path $home_,$localAppData | Out-Null
    $homeDrive = Split-Path -Qualifier $home_
    if (-not $homeDrive) { throw "test home must have a drive qualifier: $home_" }
    $env:HOMEDRIVE = $homeDrive
    $env:HOMEPATH = $home_.Substring($homeDrive.Length)
    $env:USERPROFILE = $home_
    $env:LOCALAPPDATA = $localAppData
    Remove-Item Env:\HOME, Env:\PIDASH_INSTALL_DIR -ErrorAction SilentlyContinue
    Set-Variable -Name HOME -Value $home_ -Force

    $installer = [scriptblock]::Create((Get-Content -Raw -LiteralPath $generatedInstaller))
    & $installer -ArtifactDownloadUrl $artifactUrl -NoModifyPath

    $expectedBin = Join-Path $HOME '.local\bin\pidash.exe'
    if (-not (Test-Path -PathType Leaf -LiteralPath $expectedBin)) {
        throw "generated installer did not install pidash.exe under `$HOME: $expectedBin"
    }
    Write-Host "ok generated installer under PowerShell $($PSVersionTable.PSVersion)"
} finally {
    Set-Variable -Name HOME -Value $originalPowerShellHome -Force
    $env:USERPROFILE = $originalUserProfile
    $env:HOMEDRIVE = $originalHomeDrive
    $env:HOMEPATH = $originalHomePath
    $env:LOCALAPPDATA = $originalLocalAppData
    if ($null -eq $originalEnvHome) { Remove-Item Env:\HOME -ErrorAction SilentlyContinue } else { $env:HOME = $originalEnvHome }
    Remove-Item Env:\PIDASH_INSTALL_DIR -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force -LiteralPath $work -ErrorAction SilentlyContinue
}
