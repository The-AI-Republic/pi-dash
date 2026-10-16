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
#
# `irm | iex` runs this text inside the *caller's* PowerShell session, so
# this script must never call `exit` there — that closes the user's
# window and hides any error they needed to read. Everything runs in the
# script block below, which reports failure through $LASTEXITCODE and
# `return`; only a file invocation (`powershell -File install.ps1`) turns
# that into a process exit code. Preferences like $ErrorActionPreference
# stay scoped to the block instead of leaking into the user's session; the
# one deliberate change to the session is putting pidash on its PATH.
#
# Test seam: PIDASH_INSTALLER_URL overrides the cargo-dist installer
# location (an https URL or a local file path). See
# runner/tests/install_ps1_test.ps1.

& {
    $ErrorActionPreference = 'Stop'

    $installerUrl = 'https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.ps1'
    if ($env:PIDASH_INSTALLER_URL) {
        $installerUrl = $env:PIDASH_INSTALLER_URL
    }

    Write-Host '==> Downloading pidash...'
    try {
        if (Test-Path -PathType Leaf -LiteralPath $installerUrl) {
            $installerScript = Get-Content -Raw -LiteralPath $installerUrl
        } else {
            $installerScript = Invoke-RestMethod $installerUrl
        }
    } catch {
        Write-Host ''
        Write-Host "Could not download the installer from $installerUrl"
        Write-Host "  $_"
        $global:LASTEXITCODE = 1
        return
    }

    # The cargo-dist installer ends every failure with `exit 1`. Run it in a
    # child PowerShell so that exit only ends the child: the user keeps their
    # window, sees the installer's own error text, and we read its exit code.
    # The child still evaluates the script with Invoke-Expression — exactly
    # what `irm | iex` did before — so execution-policy handling and the
    # installer's own policy check behave the same as a direct iex.
    $shellExe = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh.exe' } else { 'powershell.exe' }
    $shell = Join-Path $PSHOME $shellExe
    $scriptPath = Join-Path ([System.IO.Path]::GetTempPath()) "pidash-installer-$([guid]::NewGuid()).ps1"
    Set-Content -LiteralPath $scriptPath -Value $installerScript -Encoding UTF8
    $hadInstallerScript = Test-Path Env:\PIDASH_INSTALLER_SCRIPT
    $originalInstallerScript = $env:PIDASH_INSTALLER_SCRIPT
    $env:PIDASH_INSTALLER_SCRIPT = $scriptPath
    try {
        & $shell -NoProfile -Command 'Invoke-Expression (Get-Content -Raw -LiteralPath $env:PIDASH_INSTALLER_SCRIPT)'
        $installerExit = $LASTEXITCODE
    } finally {
        Remove-Item -LiteralPath $scriptPath -ErrorAction SilentlyContinue
        if ($hadInstallerScript) {
            $env:PIDASH_INSTALLER_SCRIPT = $originalInstallerScript
        } else {
            Remove-Item Env:\PIDASH_INSTALLER_SCRIPT -ErrorAction SilentlyContinue
        }
    }
    if ($installerExit -ne 0) {
        Write-Host ''
        Write-Host "The pidash installer failed (exit code $installerExit). See the error above."
        Write-Host 'You can also install with the MSI instead:'
        Write-Host '  https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-x86_64-pc-windows-msvc.msi'
        $global:LASTEXITCODE = $installerExit
        return
    }

    # cargo-dist drops pidash.exe into PowerShell's home directory under .local\bin
    # (install-path = "~/.local/bin" in dist-workspace.toml), or into
    # PIDASH_INSTALL_DIR when the user overrides it. If a future release
    # moves it, surface a clear error instead of silently continuing.
    $installDir = if ($env:PIDASH_INSTALL_DIR) { $env:PIDASH_INSTALL_DIR } else { Join-Path $HOME '.local\bin' }
    $pidashBin = Join-Path $installDir 'pidash.exe'
    if (-not (Test-Path -PathType Leaf -LiteralPath $pidashBin)) {
        Write-Host ''
        Write-Host "pidash.exe not found at $pidashBin after install."
        Write-Host "Run ``$installerUrl`` manually, then ``pidash auth login``."
        $global:LASTEXITCODE = 1
        return
    }

    # An older copy on PATH — typically the MSI's Program Files\pidash\bin,
    # which the MSI adds to the *system* PATH that Windows searches before
    # the user PATH cargo-dist just edited — would silently keep running in
    # new terminals, so name it and say how to remove it. Only copies on
    # PATH can shadow the new one (the MSI's PATH feature is optional), so
    # PATH is what we search. Checked before the PATH change below so
    # Get-Command still sees the pre-install PATH.
    $pidashFull = [System.IO.Path]::GetFullPath($pidashBin)
    $otherCopies = @(
        Get-Command pidash.exe -All -CommandType Application -ErrorAction SilentlyContinue |
            ForEach-Object { $_.Source }
    ) | Where-Object { $_ } |
        ForEach-Object { [System.IO.Path]::GetFullPath($_) } |
        Where-Object { $_ -ne $pidashFull } |
        Sort-Object -Unique
    if ($otherCopies) {
        Write-Host ''
        Write-Host 'Warning: another copy of pidash is also installed:'
        foreach ($copy in $otherCopies) { Write-Host "  $copy" }
        Write-Host 'New terminals may run that copy instead of the one just installed.'
        Write-Host 'If it came from the MSI, remove it in Settings > Apps > Installed apps > pidash.'
    }

    # Make `pidash` work in this window straight away. cargo-dist only edits
    # the PATH stored in the registry, which windows that are already open —
    # including this one — don't pick up, so the documented next step
    # (`pidash runner add`) failed with "not recognized". Putting the install
    # dir first also beats an older copy for this window. Under `irm | iex`
    # this is the one deliberate change to the caller's session.
    # pidash-install-test:begin-session-path
    $installDirFull = [System.IO.Path]::GetFullPath($installDir)
    $installDirRoot = [System.IO.Path]::GetPathRoot($installDirFull)
    $installDirIsRoot = $installDirFull -eq $installDirRoot
    if (-not $installDirIsRoot) { $installDirFull = $installDirFull.TrimEnd('\') }
    $otherEntries = @($env:Path -split ';' | Where-Object {
        if ($installDirIsRoot) {
            $_ -and $_ -ne $installDirFull
        } else {
            $_ -and $_.TrimEnd('\') -ne $installDirFull
        }
    })
    $env:Path = (@($installDirFull) + $otherEntries) -join ';'
    # pidash-install-test:end-session-path

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
        Write-Host 'Run `pidash auth login --no-browser`, approve the printed URL from'
        Write-Host 'another browser, then run `pidash runner add --project <PROJECT>`.'
        Write-Host '(Self-hosted Pi Dash? Add `--url <YOUR-URL>` to the login command.)'
        $global:LASTEXITCODE = 0
        return
    }

    & $pidashBin auth login
}

# Only a file invocation of *this* script may exit. Under `irm | iex`,
# $MyInvocation describes the caller — the prompt, or the caller's own
# provisioning script when iex runs inside one — so a bare
# `$MyInvocation.MyCommand.Path` check would exit someone else's script.
# Match on this file's own contents instead (the marker comment on the next
# line only exists in install.ps1); otherwise leave $LASTEXITCODE for the
# caller.
# pidash-install.ps1:self
if ($MyInvocation.MyCommand.CommandType -eq 'ExternalScript' -and
    $MyInvocation.MyCommand.ScriptContents -match 'pidash-install\.ps1:self') {
    exit $LASTEXITCODE
}
