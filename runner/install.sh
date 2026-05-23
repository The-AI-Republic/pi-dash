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

echo "==> Downloading pidash..."
curl --proto '=https' --tlsv1.2 -LsSf "$INSTALLER_URL" | sh

# cargo-dist drops the binary into $HOME/.local/bin (install-path in
# dist-workspace.toml). If a future release moves it, surface a clear
# error instead of silently continuing.
PIDASH_BIN="$HOME/.local/bin/pidash"
if [ ! -x "$PIDASH_BIN" ]; then
  echo ""
  echo "pidash binary not found at $PIDASH_BIN after install."
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
echo "Run \`pidash auth login\` from a terminal, or use the headless"
echo "enrollment path: \`pidash connect --url <URL> --token <TOKEN>\`."
