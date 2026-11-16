// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI install catalogue (NEWFRONT-555, RUN-041). The three blocks are plain
// strings pointing at the releases latest channel; nothing is fetched at
// render time. The Windows script keeps its own download link because the
// MSI cannot be piped through a shell.

export const RELEASES_LATEST = "https://github.com/The-AI-Republic/pi-dash/releases/latest/download";

export const INSTALL_UNIX_COMMAND = `curl --proto '=https' --tlsv1.2 -LsSf \\
  ${RELEASES_LATEST}/install.sh | sh`;

export const INSTALL_POWERSHELL_COMMAND = `irm ${RELEASES_LATEST}/install.ps1 | iex`;

export const WINDOWS_MSI_FILENAME = "pidash-x86_64-pc-windows-msvc.msi";
export const WINDOWS_MSI_URL = `${RELEASES_LATEST}/${WINDOWS_MSI_FILENAME}`;

export const INSTALL_MSI_COMMAND = `$ProgressPreference = "SilentlyContinue"
$msi = Join-Path $env:TEMP "${WINDOWS_MSI_FILENAME}"
Invoke-WebRequest -Uri "${WINDOWS_MSI_URL}" -OutFile $msi
Start-Process msiexec.exe -Wait -ArgumentList "/i \`"$msi\`""`;

export interface InstallBlock {
  /** Platform label; the card's first span carries it verbatim. */
  label: string;
  command: string;
  /** Direct download target, only for the MSI block. */
  download: string | null;
}

export const INSTALL_BLOCKS: InstallBlock[] = [
  { label: "macOS / Linux", command: INSTALL_UNIX_COMMAND, download: null },
  { label: "Windows (PowerShell)", command: INSTALL_POWERSHELL_COMMAND, download: null },
  { label: "Windows (MSI)", command: INSTALL_MSI_COMMAND, download: WINDOWS_MSI_URL },
];

export const INSTALL_HEADING = "Install the pidash CLI";
export const INSTALL_INTRO =
  "Run an installer on the machine that will host your AI agent. Each wrapper downloads the latest signed binary, drops `pidash` on your PATH, and starts the device-code login.";
export const INSTALL_PREREQUISITE =
  "Prerequisite: the agent CLI you plan to use (`codex` or `claude`) must already be installed and on PATH. Run `pidash doctor` after install to verify.";
export const COPY_COMMAND_LABEL = "Copy command";
export const COPIED_LABEL = "Copied!";
export const COPY_FAILED_TITLE = "Error!";
export const COPY_FAILED_BODY = "Could not copy to clipboard";
/** How long the copy control keeps its confirmation before reverting. */
export const COPY_CONFIRM_MS = 2_000;
