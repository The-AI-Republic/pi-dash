// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Panel model for the integrations tab (parity/specs/profile.md §11,
// PROF-025). The status read names every workspace the member administers;
// the panel shows one at a time and derives its badges, status fields and
// controls from that workspace's installation alone.

import type { GithubAppInstallation, GithubAppStatus, GithubAppWorkspace } from "@pidash/api-client";

import { ALL_REPOSITORIES, NEVER_STAMP, SELECTED_REPOSITORIES, STATUS_FIELD_LABELS, UNKNOWN_VALUE } from "./copy.js";

/** The workspace the panel shows: the picked one, else the first listed. */
export function selectedWorkspace(
  status: GithubAppStatus | undefined,
  slug: string | null
): GithubAppWorkspace | undefined {
  if (status === undefined) return undefined;
  const picked = slug === null ? undefined : status.workspaces.find((row) => row.slug === slug);
  return picked ?? status.workspaces[0];
}

/** An installation counts as connected only when the server says so. */
export function isConnected(installation: GithubAppInstallation | undefined): boolean {
  return installation?.connected === true;
}

/** A suspended installation still has its identifiers but cannot be used. */
export function isSuspended(installation: GithubAppInstallation | undefined): boolean {
  return typeof installation?.suspended_at === "string" && installation.suspended_at !== "";
}

/** The install control stays inert until the instance is configured. */
export function installDisabled(configured: boolean | undefined): boolean {
  return configured !== true;
}

/** The install control offers a reconnect once something is installed. */
export function installLabel(connected: boolean): string {
  return connected ? "Reconnect" : "Install";
}

/** The refresh control appears only for a connected installation. */
export function refreshVisible(installation: GithubAppInstallation | undefined): boolean {
  return isConnected(installation);
}

/** One formatted stamp, or "Never" when the host never reported one. */
export function formatStamp(value: string | null | undefined): string {
  if (value === null || value === undefined || value === "") return NEVER_STAMP;
  const when = new Date(value);
  return Number.isNaN(when.getTime()) ? NEVER_STAMP : when.toLocaleString();
}

export interface StatusField {
  label: string;
  value: string;
}

/** The identifier grid a connected panel renders, in display order. */
export function statusFields(installation: GithubAppInstallation | undefined): StatusField[] {
  if (!isConnected(installation)) return [];
  const row = installation as GithubAppInstallation;
  return [
    { label: STATUS_FIELD_LABELS.account, value: row.account_login || UNKNOWN_VALUE },
    { label: STATUS_FIELD_LABELS.accountType, value: row.account_type || UNKNOWN_VALUE },
    {
      label: STATUS_FIELD_LABELS.repositoryAccess,
      value: row.repository_selection === "all" ? ALL_REPOSITORIES : SELECTED_REPOSITORIES,
    },
    { label: STATUS_FIELD_LABELS.repositoryCount, value: String(row.repository_count ?? 0) },
    { label: STATUS_FIELD_LABELS.verified, value: formatStamp(row.verified_at) },
    { label: STATUS_FIELD_LABELS.lastChecked, value: formatStamp(row.last_checked_at) },
  ];
}

/** The failure banner's text, or null when the last check passed. */
export function lastCheckError(installation: GithubAppInstallation | undefined): string | null {
  const detail = installation?.last_check_error ?? "";
  return detail === "" ? null : detail;
}
