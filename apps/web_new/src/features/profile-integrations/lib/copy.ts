// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Wording for the integrations tab (parity/specs/profile.md §11, PROF-025).
// The headings, banners, status-field labels, install/refresh labels and
// callback notices are pinned by the oracle.

export const INTEGRATIONS_TAB_HEADING = "Integrations";
export const INTEGRATIONS_TAB_INTRO = "Connect external services to workspaces you administer.";

export const WORKSPACE_LABEL = "Workspace";

export const NO_ADMIN_HEADING = "No admin workspaces";
export const NO_ADMIN_COPY = "You need workspace admin access before you can install GitHub for a Pi Dash workspace.";

export const GITHUB_PANEL_HEADING = "GitHub App";
export const CONFIG_MISSING_WARNING = "GitHub App configuration is missing on this Pi Dash instance.";
export const UNCONNECTED_COPY =
  "No GitHub App installation is connected for this workspace yet. Pi Dash will verify installation access after GitHub redirects you back.";
export const SUSPENDED_BANNER = "This GitHub App installation is suspended or removed in GitHub.";
export const LAST_CHECK_FAILED_PREFIX = "Last check failed:";

export const INSTALL_LABEL = "Install";
export const RECONNECT_LABEL = "Reconnect";
export const REFRESH_LABEL = "Refresh";

export const INSTALL_FAILED_TITLE = "Install failed";
export const INSTALL_FAILED_FALLBACK = "Could not start the GitHub App install flow.";
export const REFRESH_OK_TITLE = "GitHub refreshed";
export const REFRESH_OK_MESSAGE = "The GitHub App installation was checked successfully.";
export const REFRESH_FAILED_TITLE = "Refresh failed";
export const REFRESH_FAILED_FALLBACK = "Could not check the GitHub App installation.";

export const CALLBACK_OK_TITLE = "GitHub connected";
export const CALLBACK_OK_MESSAGE = "The GitHub App installation was verified.";
export const CALLBACK_FAILED_TITLE = "GitHub connection failed";
export const CALLBACK_FAILED_FALLBACK = "The GitHub App installation could not be completed.";

export const STATUS_FIELD_LABELS = {
  account: "Account",
  accountType: "Account type",
  repositoryAccess: "Repository access",
  repositoryCount: "Repositories visible",
  verified: "Verified",
  lastChecked: "Last checked",
} as const;

export const NEVER_STAMP = "Never";
export const UNKNOWN_VALUE = "Unknown";
export const ALL_REPOSITORIES = "All repositories";
export const SELECTED_REPOSITORIES = "Selected repositories";

/** What the panel says once installed: who, where. */
export function installedLead(account: string, workspace: string): string {
  return `Installed for ${account} on ${workspace}.`;
}

/** What the panel says before installation. */
export function installLead(workspace: string): string {
  return `Install the Pi Dash GitHub App for ${workspace}.`;
}
