@echo off
rem Pi Dash post-install setup shim.
rem
rem Launched from the MSI ExitDialog checkbox and the Start Menu shortcut so
rem the device-code login runs unelevated as the installing user (the token
rem and workspace binding land under that user's profile, not SYSTEM's).
rem
rem pidash.exe is a console-subsystem app; ShellExecute-ing it directly would
rem allocate a console that vanishes the instant the process exits, so a
rem failed login would show nothing. Running it from this shim keeps the
rem window open via `pause` for the sign-in flow and any error output.
rem
rem %~dp0 expands to this script's directory (the install bin dir) with a
rem trailing backslash, so the sibling pidash.exe is invoked by full path.
"%~dp0pidash.exe" auth login
echo.
pause
