@echo off
REM Pi Dash post-install sign-in helper.
REM
REM Launched two ways, both as the *installing user* rather than the Windows
REM Installer service: from the MSI ExitDialog checkbox, and from the Start
REM Menu shortcut. That distinction matters — `pidash auth login` writes the
REM machine token and workspace binding under the user's profile, so running
REM it as SYSTEM would file the credentials against the wrong account.
REM
REM Exists as a .cmd rather than invoking pidash.exe directly so the console
REM window survives the process exiting; otherwise a failed login flashes and
REM disappears before the user can read it.
setlocal

echo ==^> Starting Pi Dash sign-in...
echo.

"%~dp0pidash.exe" auth login
set "_PIDASH_RC=%ERRORLEVEL%"

echo.
if not "%_PIDASH_RC%"=="0" (
  echo Sign-in did not complete ^(exit code %_PIDASH_RC%^).
  echo You can retry at any time with:
  echo.
  echo     pidash auth login
  echo.
) else (
  echo Next: register a runner for a project with:
  echo.
  echo     pidash runner add --project ^<PROJECT^>
  echo.
)

pause
endlocal
