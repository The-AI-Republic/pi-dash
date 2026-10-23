# Regression tests for runner/install.ps1.
#
# The installer is documented as `irm .../install.ps1 | iex`, which runs it
# inside the user's own PowerShell session. It must never end that session
# (a bare `exit` closes the user's window and hides the error they needed to
# read), must surface installer errors, must report failure through
# $LASTEXITCODE, and must not leak preferences into the session. Run as a
# file (`powershell -File install.ps1`), it must still exit with a code.
#
# Each case runs in a fresh child shell against a fake cargo-dist installer
# (via the PIDASH_INSTALLER_URL seam) and a fake pidash.exe, with
# PowerShell's home variables pointed at a temp directory. Nothing is downloaded or
# installed for real.
#
# Usage (Windows only):
#   powershell -NoProfile -File runner/tests/install_ps1_test.ps1
#   pwsh       -NoProfile -File runner/tests/install_ps1_test.ps1
# Optional: -Shells to choose which shells run the cases (default: both
# Windows PowerShell and pwsh when available); -InstallScript to test a
# different copy of install.ps1.

param(
    [string[]] $Shells,
    [string] $InstallScript
)

$ErrorActionPreference = 'Stop'

if (-not $InstallScript) { $InstallScript = Join-Path $PSScriptRoot '..\install.ps1' }
$installPs1 = (Resolve-Path $InstallScript).Path
$originalUserProfile = $env:USERPROFILE
$originalHomeDrive = $env:HOMEDRIVE
$originalHomePath = $env:HOMEPATH
$originalEnvHome = $env:HOME
$originalPath = $env:Path
$hadOriginalInstallerScript = Test-Path Env:\PIDASH_INSTALLER_SCRIPT
$originalInstallerScript = $env:PIDASH_INSTALLER_SCRIPT
$work = Join-Path ([System.IO.Path]::GetTempPath()) "pidash-install-test-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $work | Out-Null
try {

if (-not $Shells) {
    $Shells = @("$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe")
    $pwsh = Get-Command pwsh.exe -ErrorAction SilentlyContinue
    if ($pwsh) { $Shells += $pwsh.Source }
}

# ---------------------------------------------------------------- fixtures

# Fake pidash.exe: echoes its arguments and exits with FAKE_PIDASH_EXIT.
# Compiled with Windows PowerShell's Add-Type, which can emit a console exe.
$fakeExe = Join-Path $work 'fake-pidash.exe'
$fakeSource = @'
public static class FakePidash {
    public static int Main(string[] args) {
        System.Console.WriteLine("fake-pidash: " + string.Join(" ", args));
        string code = System.Environment.GetEnvironmentVariable("FAKE_PIDASH_EXIT");
        return string.IsNullOrEmpty(code) ? 0 : int.Parse(code);
    }
}
'@
$compile = "Add-Type -TypeDefinition @'`n$fakeSource`n'@ -OutputAssembly '$fakeExe' -OutputType ConsoleApplication"
$encodedCompile = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($compile))
& "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -EncodedCommand $encodedCompile
if (-not (Test-Path $fakeExe)) { throw "could not build $fakeExe" }

function New-FakeInstaller([string] $name, [string] $body) {
    $path = Join-Path $work "$name.ps1"
    Set-Content -LiteralPath $path -Value $body -Encoding UTF8
    return $path
}

# Mirrors cargo-dist's generated installer: on success it places the binary
# (in PIDASH_INSTALL_DIR when set, else PowerShell's $HOME/.local/bin), on failure
# it prints the error and calls `exit 1`.
$installerOk = New-FakeInstaller 'installer-ok' @"
if (`$env:PIDASH_TEST_HOME) { Set-Variable -Name HOME -Value `$env:PIDASH_TEST_HOME -Force }
`$dest = if (`$env:PIDASH_INSTALL_DIR) { `$env:PIDASH_INSTALL_DIR } else { Join-Path `$HOME '.local\bin' }
New-Item -ItemType Directory -Force -Path `$dest | Out-Null
Copy-Item -LiteralPath '$fakeExe' -Destination (Join-Path `$dest 'pidash.exe')
Write-Host 'fake-installer: installed'
"@
$installerFails = New-FakeInstaller 'installer-fails' @'
$InformationPreference = "Continue"
try {
  throw "fake-installer: boom"
} catch {
  Write-Information $_
  exit 1
}
'@
$installerNoBinary = New-FakeInstaller 'installer-no-binary' "Write-Host 'fake-installer: nothing installed'"
$unreachable = 'http://127.0.0.1:9/pidash-installer.ps1'

# ---------------------------------------------------------------- harness

$failures = New-Object System.Collections.Generic.List[string]
$passed = 0

function Invoke-Child([string] $shell, [string] $command) {
    # Windows PowerShell turns a child's stderr lines into terminating errors
    # under 'Stop'; the child's output is data here, not a harness failure.
    $ErrorActionPreference = 'Continue'
    $encoded = [Convert]::ToBase64String([System.Text.Encoding]::Unicode.GetBytes($command))
    $out = & $shell -NoProfile -EncodedCommand $encoded 2>&1 | Out-String
    return [pscustomobject]@{ Output = $out; ExitCode = $LASTEXITCODE }
}

# Every case gets its own profile and a minimal PATH, so a real pidash
# already installed on the test machine can't leak in. -MsiCopy puts a fake
# older copy on PATH, the way the MSI adds Program Files\pidash\bin to it.
# (Program Files itself can't be faked: Windows resets ProgramFiles in every
# new process, which is also why install.ps1 searches PATH instead.)
function Set-CaseEnv([string] $installer, [int] $pidashExit, [string] $installDir,
                     [switch] $MsiCopy, [switch] $DifferentUserProfile) {
    $home_ = Join-Path $work "home-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Path $home_ | Out-Null
    $homeDrive = Split-Path -Qualifier $home_
    if (-not $homeDrive) { throw "test home must have a drive qualifier: $home_" }
    $env:HOMEDRIVE = $homeDrive
    $env:HOMEPATH = $home_.Substring($homeDrive.Length)
    $env:USERPROFILE = if ($DifferentUserProfile) {
        $differentProfile = Join-Path $work "profile-$([guid]::NewGuid())"
        New-Item -ItemType Directory -Path $differentProfile | Out-Null
        $differentProfile
    } else {
        $home_
    }
    # cargo-dist's former `$env:HOME` rendering failed on normal Windows
    # machines because this environment variable is ordinarily absent.
    Remove-Item Env:\HOME -ErrorAction SilentlyContinue
    $env:Path = "$env:SystemRoot\System32;$env:SystemRoot"
    if ($MsiCopy) {
        $msiBin = Join-Path $work "msi-$([guid]::NewGuid())\pidash\bin"
        New-Item -ItemType Directory -Path $msiBin | Out-Null
        Copy-Item -LiteralPath $fakeExe -Destination (Join-Path $msiBin 'pidash.exe')
        $env:Path = "$msiBin;$env:Path"
    }
    $env:PIDASH_INSTALLER_URL = $installer
    $env:FAKE_PIDASH_EXIT = "$pidashExit"
    $script:caseInstallerScript = "existing-$([guid]::NewGuid())"
    $env:PIDASH_INSTALLER_SCRIPT = $script:caseInstallerScript
    if ($DifferentUserProfile) { $env:PIDASH_TEST_HOME = $home_ } else { Remove-Item Env:\PIDASH_TEST_HOME -ErrorAction SilentlyContinue }
    if ($installDir) { $env:PIDASH_INSTALL_DIR = $installDir } else { Remove-Item Env:\PIDASH_INSTALL_DIR -ErrorAction SilentlyContinue }
    $script:caseHome = $home_
}

# Runs install.ps1 exactly like `irm | iex`, then proves the session is still
# alive and reports what the caller can observe afterwards. The try/catch
# models an interactive prompt, where an uncaught error prints and the
# session carries on — only `exit` actually ends it.
function Invoke-ViaIex([string] $shell) {
    $cmd = @"
if (`$env:PIDASH_TEST_HOME) { Set-Variable -Name HOME -Value `$env:PIDASH_TEST_HOME -Force }
try { Get-Content -Raw -LiteralPath '$installPs1' | Invoke-Expression } catch { Write-Host "UNCAUGHT: `$_" }
Write-Host "SESSION-ALIVE exit=`$LASTEXITCODE eap=`$ErrorActionPreference interactive=`$([Environment]::UserInteractive)"
Write-Host "SESSION-PATH home=<`$HOME> profile=<`$env:USERPROFILE> firstpath=<`$((`$env:Path -split ';')[0])>"
Write-Host "SESSION-INSTALLER-SCRIPT <`$env:PIDASH_INSTALLER_SCRIPT>"
"@
    return Invoke-Child $shell $cmd
}

function Assert-Case([string] $name, [bool] $condition, [string] $detail) {
    if ($condition) {
        $script:passed++
        Write-Host "  ok   $name"
    } else {
        $script:failures.Add("$name`n$detail")
        Write-Host "  FAIL $name"
    }
}

# Execute the wrapper's actual session-PATH block in isolation. Installing a
# fake executable at C:\ would require administrator access, but path handling
# itself does not: the old TrimEnd implementation turned C:\ into drive-relative
# C:, and this check catches that regression directly.
function Test-DriveRootPathUpdate {
    $source = Get-Content -Raw -LiteralPath $installPs1
    $match = [regex]::Match(
        $source,
        '(?s)# pidash-install-test:begin-session-path\r?\n(?<body>.*?)\r?\n\s*# pidash-install-test:end-session-path'
    )
    if (-not $match.Success) { throw 'could not find install.ps1 session-PATH test block' }

    $savedPath = $env:Path
    try {
        $installDir = [System.IO.Path]::GetPathRoot($work)
        $env:Path = "$installDir;$env:SystemRoot\System32"
        & ([scriptblock]::Create($match.Groups['body'].Value))
        $pathEntries = @($env:Path -split ';')
        Assert-Case 'drive-root install dir / root remains absolute' ($pathEntries[0] -eq $installDir) $env:Path
        Assert-Case 'drive-root install dir / duplicate removed' `
            (@($pathEntries | Where-Object { $_ -eq $installDir }).Count -eq 1) $env:Path
    } finally {
        $env:Path = $savedPath
    }
}

function Test-IexCase([string] $shell, [string] $name, [string] $installer, [int] $pidashExit,
                      [int] $expectExit, [string[]] $expectText, [switch] $NeedsInteractive,
                      [string] $InstallDir, [switch] $MsiCopy, [switch] $ExpectOnPath,
                      [string[]] $RejectText, [switch] $DifferentUserProfile) {
    Set-CaseEnv $installer $pidashExit $InstallDir -MsiCopy:$MsiCopy -DifferentUserProfile:$DifferentUserProfile
    $expectedDir = if ($InstallDir) { $InstallDir } else { Join-Path $script:caseHome '.local\bin' }
    $r = Invoke-ViaIex $shell
    $alive = $r.Output -match 'SESSION-ALIVE exit=(-?\d+) eap=(\w+) interactive=(\w+)'
    if (-not $alive) {
        Assert-Case "$name / session survives" $false $r.Output
        return
    }
    $exit, $eap, $interactive = [int]$Matches[1], $Matches[2], ($Matches[3] -eq 'True')
    $pathReported = $r.Output -match 'SESSION-PATH home=<(.*)> profile=<(.*)> firstpath=<(.*)>'
    if ($pathReported) {
        $reportedHome, $reportedProfile, $firstPath = $Matches[1], $Matches[2], $Matches[3]
    } else {
        $reportedHome, $reportedProfile, $firstPath = $null, $null, $null
    }
    Assert-Case "$name / session survives" $true ''
    Assert-Case "$name / session paths reported" $pathReported $r.Output
    Assert-Case "$name / no uncaught error" ($r.Output -notmatch 'UNCAUGHT:') $r.Output
    Assert-Case "$name / ErrorActionPreference not leaked" ($eap -eq 'Continue') $r.Output
    Assert-Case "$name / existing installer variable restored" `
        ($r.Output -match "SESSION-INSTALLER-SCRIPT <$([regex]::Escape($script:caseInstallerScript))>") $r.Output
    if ($DifferentUserProfile) {
        Assert-Case "$name / HOME differs from USERPROFILE" ($reportedHome -ne $reportedProfile) $r.Output
        Assert-Case "$name / wrapper follows HOME" ($reportedHome -eq $script:caseHome) $r.Output
    }
    if ($ExpectOnPath) {
        # Checked before the interactive split: the PATH fix happens either way.
        Assert-Case "$name / install dir is first on this window's PATH" ($firstPath -eq $expectedDir) "$firstPath`n$($r.Output)"
    }
    foreach ($t in $RejectText) {
        Assert-Case "$name / does not show '$t'" ($r.Output -notmatch [regex]::Escape($t)) $r.Output
    }
    if ($NeedsInteractive -and -not $interactive) {
        # No desktop session: install.ps1 takes its headless branch instead
        # of running `pidash auth login`, which must still succeed quietly.
        Assert-Case "$name / headless branch exits 0" ($exit -eq 0 -and $r.Output -match 'skipping auto-auth') $r.Output
        return
    }
    Assert-Case "$name / `$LASTEXITCODE = $expectExit" ($exit -eq $expectExit) $r.Output
    foreach ($t in $expectText) {
        Assert-Case "$name / shows '$t'" ($r.Output -match [regex]::Escape($t)) $r.Output
    }
}

# ---------------------------------------------------------------- cases

    Test-DriveRootPathUpdate

    foreach ($shell in $Shells) {
        Write-Host ''
        Write-Host "== $shell"

        Test-IexCase $shell 'installer fails' $installerFails 0 1 @('fake-installer: boom', 'The pidash installer failed')
        Test-IexCase $shell 'download fails' $unreachable 0 1 @('Could not download the installer')
        Test-IexCase $shell 'binary missing after install' $installerNoBinary 0 1 @('pidash.exe not found')
        Test-IexCase $shell 'install + login succeed' $installerOk 0 0 @('fake-installer: installed', 'fake-pidash: auth login') -NeedsInteractive `
            -ExpectOnPath -RejectText @('another copy of pidash')
        Test-IexCase $shell 'login fails' $installerOk 7 7 @('fake-pidash: auth login') -NeedsInteractive
        # The wrapper must look where cargo-dist installed, not a fixed path.
        Test-IexCase $shell 'PIDASH_INSTALL_DIR override' $installerOk 0 0 @('fake-pidash: auth login') -NeedsInteractive `
            -InstallDir (Join-Path $work "custom-dir-$([guid]::NewGuid())") -ExpectOnPath
        # Windows PowerShell 5.1 can derive $HOME separately from USERPROFILE.
        # Keep them deliberately different so the fake installer and wrapper
        # must agree on cargo-dist's actual $HOME destination.
        if ([System.IO.Path]::GetFileName($shell) -ieq 'powershell.exe') {
            Test-IexCase $shell 'HOME differs from USERPROFILE' $installerOk 0 0 @('fake-installer: installed') `
                -DifferentUserProfile -ExpectOnPath
        }
        # An MSI copy on the system PATH would shadow the new install: warn.
        Test-IexCase $shell 'older MSI copy present' $installerOk 0 0 @('another copy of pidash', 'pidash\bin\pidash.exe', 'Installed apps') -NeedsInteractive `
            -MsiCopy -ExpectOnPath

        # iex from inside the caller's own script must not exit that script.
        Set-CaseEnv $installerFails 0
        $caller = Join-Path $work 'caller.ps1'
        Set-Content -LiteralPath $caller -Encoding UTF8 -Value @"
Get-Content -Raw -LiteralPath '$installPs1' | Invoke-Expression
Write-Host "CALLER-CONTINUED exit=`$LASTEXITCODE"
"@
        $r = Invoke-Child $shell "& '$caller'"
        Assert-Case 'iex inside a caller script / caller keeps running' ($r.Output -match 'CALLER-CONTINUED exit=1') $r.Output

        # Run as a file, the script must still exit with the failure code.
        Set-CaseEnv $installerFails 0
        $out = & $shell -NoProfile -File $installPs1 2>&1 | Out-String
        Assert-Case 'run as a file / exits 1 on installer failure' ($LASTEXITCODE -eq 1) $out
    }
} finally {
    $env:USERPROFILE = $originalUserProfile
    $env:HOMEDRIVE = $originalHomeDrive
    $env:HOMEPATH = $originalHomePath
    if ($null -eq $originalEnvHome) { Remove-Item Env:\HOME -ErrorAction SilentlyContinue } else { $env:HOME = $originalEnvHome }
    $env:Path = $originalPath
    if ($hadOriginalInstallerScript) {
        $env:PIDASH_INSTALLER_SCRIPT = $originalInstallerScript
    } else {
        Remove-Item Env:\PIDASH_INSTALLER_SCRIPT -ErrorAction SilentlyContinue
    }
    Remove-Item Env:\PIDASH_INSTALLER_URL, Env:\FAKE_PIDASH_EXIT, Env:\PIDASH_INSTALL_DIR, Env:\PIDASH_TEST_HOME -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force -LiteralPath $work -ErrorAction SilentlyContinue
}

Write-Host ''
if ($failures.Count -gt 0) {
    Write-Host "$($failures.Count) failed, $passed passed"
    foreach ($f in $failures) { Write-Host "---- $f" }
    exit 1
}
Write-Host "all $passed checks passed"
exit 0
