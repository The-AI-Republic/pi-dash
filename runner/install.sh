#!/usr/bin/env sh
# Pi Dash runner installer.
#
# Wraps the cargo-dist-generated `pidash-installer.sh` and then
# launches `pidash auth login` so the user lands in the device-code
# flow on the same install one-liner. The runner is a "set up once,
# forget" daemon driven by Pi Dash cloud, so the natural moment to
# authenticate is right now while the user is at the terminal — not
# the first time they happen to type `pidash` themselves (which may
# be never).
#
# Usage:
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.sh | sh
set -eu

INSTALLER_URL="https://github.com/The-AI-Republic/pi-dash/releases/latest/download/pidash-installer.sh"

# Running under WSL installs the *Linux* build, which is usually not what a
# Windows user wants: it cannot see the Windows filesystem the way they expect
# and cannot drive coding agents installed on the Windows side. The install
# still succeeds, so warn loudly rather than failing — a runner that genuinely
# lives inside the WSL distro is a legitimate setup.
if grep -qi microsoft /proc/version 2>/dev/null || [ -n "${WSL_DISTRO_NAME:-}" ]; then
  echo ""
  echo "Warning: this looks like WSL, so the Linux build of pidash will be installed."
  echo "It will drive coding agents inside this WSL distro only — not ones installed"
  echo "on Windows. If your agents live on the Windows side, cancel and run this"
  echo "from PowerShell instead:"
  echo ""
  echo "  irm https://github.com/The-AI-Republic/pi-dash/releases/latest/download/install.ps1 | iex"
  echo ""

  # Actually stop and ask, rather than printing advice the user cannot act on:
  # install and `auth login` follow immediately, so a bare warning would need
  # to be read and Ctrl-C'd faster than the download completes. stdin is the
  # `curl | sh` pipe, so read from /dev/tty — probed for openability because
  # the device node exists even where it cannot be opened (Docker without -t,
  # cron, systemd, SSH without -t).
  if (: </dev/tty) 2>/dev/null; then
    printf "Continue installing the Linux build? [y/N] "
    read -r _wsl_reply </dev/tty || _wsl_reply=""
    case "$_wsl_reply" in
      [yY] | [yY][eE][sS]) ;;
      *)
        echo "Cancelled."
        exit 0
        ;;
    esac
    echo ""
  else
    # Headless: nobody to ask, and erroring out would break legitimate
    # automated installs into a WSL distro. Warn and continue.
    echo "No terminal detected — continuing with the Linux build."
    echo ""
  fi
fi

echo "==> Downloading pidash..."
curl --proto '=https' --tlsv1.2 -LsSf "$INSTALLER_URL" | sh

# cargo-dist drops the binary into $HOME/.local/bin (install-path in
# dist-workspace.toml). If a future release moves it, surface a clear
# error instead of silently continuing.
#
# Under Git Bash / MSYS2 / Cygwin the installed file is `pidash.exe`: the
# cargo-dist installer maps MINGW*/MSYS*/CYGWIN* to a Windows target and
# unpacks the Windows archive. Those runtimes usually resolve a bare
# `pidash` to `pidash.exe` transparently, but that is a property of the
# emulation layer rather than something to rely on — check both names.
PIDASH_BIN="$HOME/.local/bin/pidash"
if [ ! -x "$PIDASH_BIN" ] && [ -x "$PIDASH_BIN.exe" ]; then
  PIDASH_BIN="$PIDASH_BIN.exe"
fi
if [ ! -x "$PIDASH_BIN" ]; then
  echo ""
  echo "pidash binary not found at $HOME/.local/bin/pidash after install."
  echo "Run \`$INSTALLER_URL\` manually, then \`pidash auth login\`."
  exit 1
fi

echo ""
echo "==> Starting authentication..."
echo ""

# When this script is invoked as `curl … | sh`, sh's stdin is the curl
# pipe and an interactive prompt would see EOF. Reattach /dev/tty so
# the device-code flow can read keystrokes for the workspace and
# runner-add prompts.
#
# `[ -e /dev/tty ]` is not enough: the device node exists on disk even
# in headless contexts (Docker without -t, cron, systemd, SSH without
# -t), so the check would pass and then `< /dev/tty` would crash with
# "No such device or address". Probe openability by actually trying to
# read from /dev/tty in a subshell.
if (: </dev/tty) 2>/dev/null; then
  exec "$PIDASH_BIN" auth login < /dev/tty
fi

# No TTY (CI, Dockerfile, piped without a terminal): don't try to
# auth — there's no one to approve the device code. Point at the
# headless path instead.
echo "No terminal detected — skipping auto-auth."
echo "Run \`pidash auth login --no-browser --url <URL>\`, approve the printed"
echo "URL from another browser, then run \`pidash runner add --project <PROJECT>\`."
