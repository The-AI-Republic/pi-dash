# Pi Dash MSI lifecycle helper.
#
# Windows Installer runs this script at these points (see main.wxs):
#
#   Stop       Before InstallValidate on upgrade and uninstall, as the
#              installing user. Ends this MSI's runner and the agent processes
#              it started, so Windows does not find pidash.exe in use and ask
#              for a reboot. Leaves a marker so the runner can be put back.
#   Restart    After a successful upgrade (commit, as SYSTEM), after a failed
#              upgrade or uninstall (rollback, as SYSTEM), or after a cancel
#              (as the installing user). Starts the PiDash logon task again
#              if Stop left a marker.
#   Uninstall  Before RemoveFiles on a real uninstall, as SYSTEM. The logon
#              task is created by `pidash install`, not by Windows Installer,
#              so nothing else removes it.
#
# Processes and the task are matched on the full path of this MSI's
# pidash.exe, never on a name, so another Pi Dash installation (for example
# the cargo-dist copy in ~\.local\bin) and its task are left alone.
#
# Stop runs as the user rather than ending the task because a task created
# from an elevated prompt is read-only for the user, while the user can always
# end their own unelevated runner process.
#
# The MSI embeds this script rather than installing it: Stop runs before the
# new files exist and rollback runs after they are gone. After editing, run
# update-msi-lifecycle.ps1 to regenerate pidash-msi-lifecycle.wxi.

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Stop', 'Restart', 'Uninstall')]
    [string] $Mode,
    [Parameter(Mandatory = $true)]
    [string] $BinDir,
    [string] $StateDir = (Join-Path $env:ProgramData 'pidash'),
    [string] $TaskName = 'PiDash',
    [int] $TimeoutSeconds = 20
)

$ErrorActionPreference = 'Stop'
$exe = [System.IO.Path]::GetFullPath((Join-Path $BinDir 'pidash.exe'))
$marker = Join-Path $StateDir 'msi-restart-runner'
$logFile = Join-Path $StateDir 'msi-lifecycle.log'

# Windows Installer captures stdout for most actions, but not for the
# cancel/error recovery, so also keep a small log next to the marker.
function Write-Log([string] $message) {
    $line = "pidash-msi ${Mode}: $message"
    Write-Host $line
    try {
        New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
        Add-Content -LiteralPath $logFile -Value "$((Get-Date).ToString('o')) $line"
    } catch {
    }
}

function Test-MsiExe([string] $path) {
    if (-not $path) { return $false }
    try {
        $full = [System.IO.Path]::GetFullPath($path.Trim().Trim('"'))
    } catch {
        return $false
    }
    return [string]::Equals($full, $exe, [System.StringComparison]::OrdinalIgnoreCase)
}

# Callers wrap the result in @(): PowerShell unrolls a one-element array
# returned from a function, and a lone CIM instance has no usable .Count.
function Get-MsiRunners($processes) {
    @($processes | Where-Object {
        (Test-MsiExe $_.ExecutablePath) -and $_.CommandLine -match '(^|\s)__run(\s|$)'
    })
}

function Get-TaskTargets($task) {
    @($task.Actions | ForEach-Object { $_.Execute } | Where-Object { $_ })
}

function Test-TaskOwned($task) {
    @(Get-TaskTargets $task | Where-Object { Test-MsiExe $_ }).Count -gt 0
}

function Format-Pids($processes) {
    (@($processes | ForEach-Object { $_.ProcessId }) -join ', ')
}

function Invoke-Stop {
    $processes = @(Get-CimInstance -ClassName Win32_Process)
    $runners = @(Get-MsiRunners $processes)
    if ($runners.Count -eq 0) {
        Remove-Item -LiteralPath $marker -Force -ErrorAction SilentlyContinue
        Write-Log "no runner is running from $exe"
        return 0
    }

    # Record the restart before stopping anything: if this fails the upgrade
    # still needs the file, and the log says how to start Pi Dash again.
    try {
        New-Item -ItemType Directory -Force -Path $StateDir | Out-Null
        Set-Content -LiteralPath $marker -Value $exe
    } catch {
        Write-Log "could not record that the runner should be restarted ($($_.Exception.Message)); run 'pidash start' afterwards"
    }

    # Runners first, so they cannot start more agent processes, then
    # everything they started. A descendant must be newer than its parent, so
    # a recycled parent PID cannot pull in an unrelated process.
    $tree = New-Object System.Collections.Generic.List[object]
    foreach ($runner in $runners) { $tree.Add($runner) }
    for ($i = 0; $i -lt $tree.Count; $i++) {
        $parent = $tree[$i]
        foreach ($child in $processes) {
            if ($child.ParentProcessId -eq $parent.ProcessId -and
                $child.CreationDate -ge $parent.CreationDate -and
                -not ($tree | Where-Object { $_.ProcessId -eq $child.ProcessId })) {
                $tree.Add($child)
            }
        }
    }

    foreach ($process in $tree) {
        $kind = if ($runners -contains $process) { 'runner' } else { 'agent process' }
        if (-not (Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue)) { continue }
        try {
            Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
            Write-Log "stopped $kind $($process.Name) (pid $($process.ProcessId))"
        } catch {
            # A runner's console host exits with the runner, often in between.
            if (-not (Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue)) {
                Write-Log "$kind $($process.Name) (pid $($process.ProcessId)) already exited"
            } else {
                Write-Log "could not stop $kind $($process.Name) (pid $($process.ProcessId)): $($_.Exception.Message)"
            }
        }
    }

    # Stop-Process returns before the process is gone. Windows checks for
    # files in use right after this script, so wait until the image is free.
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $remaining = @(Get-CimInstance -ClassName Win32_Process | Where-Object { Test-MsiExe $_.ExecutablePath })
        $remainingRunners = @(Get-MsiRunners $remaining)
        if ($remainingRunners.Count -eq 0) { break }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)

    foreach ($process in $remaining) {
        if ($remainingRunners -notcontains $process) {
            Write-Log "pid $($process.ProcessId) still uses $exe but is not a runner (for example 'pidash tui'); Windows will ask about it"
        }
    }
    if ($remainingRunners.Count -gt 0) {
        Write-Log "runner still running after ${TimeoutSeconds}s (pid $(Format-Pids $remainingRunners))"
        return 1
    }
    Write-Log "no runner is using $exe any more"
    return 0
}

function Invoke-Restart {
    if (-not (Test-Path -LiteralPath $marker)) {
        Write-Log 'no runner was stopped for this install; nothing to restart'
        return 0
    }
    # Commit, rollback and cancel recovery can all run in one install. The
    # marker is cleared only after an attempt, so a helper that is killed
    # midway leaves it for the next one; the already-running check keeps the
    # runner from being started twice.
    try {
        return (Start-MsiRunner)
    } finally {
        try {
            Remove-Item -LiteralPath $marker -Force
        } catch {
            Write-Log "could not clear restart marker: $($_.Exception.Message)"
        }
    }
}

function Start-MsiRunner {
    $running = @(Get-MsiRunners @(Get-CimInstance -ClassName Win32_Process))
    if ($running.Count -gt 0) {
        Write-Log "runner already running (pid $(Format-Pids $running))"
        return 0
    }
    if (-not (Test-Path -LiteralPath $exe)) {
        Write-Log "$exe is not installed; not starting the runner"
        return 0
    }

    $task = Get-ScheduledTask -TaskName $TaskName -TaskPath '\' -ErrorAction SilentlyContinue
    if (-not $task) {
        Write-Log "no scheduled task '$TaskName'; run 'pidash start' to start the runner"
        return 0
    }
    if (-not (Test-TaskOwned $task)) {
        Write-Log "task '$TaskName' starts $((Get-TaskTargets $task) -join ', '), not $exe; leaving it"
        return 0
    }
    if ($task.Settings -and $task.Settings.Enabled -eq $false) {
        Write-Log "task '$TaskName' is disabled; not starting it"
        return 0
    }

    try {
        Start-ScheduledTask -InputObject $task -ErrorAction Stop
    } catch {
        Write-Log "could not start task '$TaskName' ($($_.Exception.Message)); run 'pidash start' from an administrator PowerShell"
        return 1
    }

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $running = @(Get-MsiRunners @(Get-CimInstance -ClassName Win32_Process))
        if ($running.Count -gt 0) {
            Write-Log "runner started (pid $(Format-Pids $running))"
            return 0
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    Write-Log "task '$TaskName' was started, but no runner from $exe appeared within ${TimeoutSeconds}s; check 'pidash status'"
    return 1
}

function Invoke-Uninstall {
    Remove-Item -LiteralPath $marker -Force -ErrorAction SilentlyContinue
    try {
        $task = Get-ScheduledTask -TaskName $TaskName -TaskPath '\' -ErrorAction SilentlyContinue
        if (-not $task) {
            Write-Log "no scheduled task '$TaskName'; nothing to remove"
            return 0
        }
        if (-not (Test-TaskOwned $task)) {
            Write-Log "task '$TaskName' starts $((Get-TaskTargets $task) -join ', '), not $exe; leaving it"
            return 0
        }
        Stop-ScheduledTask -InputObject $task -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -InputObject $task -Confirm:$false -ErrorAction Stop
        Write-Log "removed scheduled task '$TaskName' for $exe"
    } catch {
        # Best-effort: a leftover task is preferable to blocking the uninstall.
        Write-Log "cleanup skipped: $($_.Exception.Message)"
    }
    return 0
}

try {
    switch ($Mode) {
        'Stop' { $code = Invoke-Stop }
        'Restart' { $code = Invoke-Restart }
        'Uninstall' { $code = Invoke-Uninstall }
    }
} catch {
    Write-Log "failed: $($_.Exception.Message)"
    $code = 1
}
exit $code
