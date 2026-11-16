// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Previous-exports history model (parity/specs/exports.md §3, EXP-006–013).
// Row facts, the seven-day download rule, and the polling decision behind
// the history section. Labels pinned by the oracle scenario
// (exports/workspace-exports.spec.ts) are behavior, not old-app copy.
import type { BadgeTone } from "@pidash/kit";
import { isKnownExportStatus, type WorkspaceExport } from "@pidash/api-client";

/** Rows per history window (EXP-010: ten per page). */
export const EXPORT_HISTORY_PAGE_SIZE = 10;

/** Opening cursor of the first history read, `limit:page:is_prev`. */
export const EXPORT_HISTORY_FIRST_CURSOR = "10:0:0";

/** Re-read cadence while any row is still processing (EXP-009). */
export const EXPORT_HISTORY_POLL_MS = 3_000;

/** Download lifetime of a finished export, in days (EXP-012). */
export const EXPORT_HISTORY_EXPIRY_DAYS = 7;

/** Same lifetime in milliseconds for the expiry comparison. */
export const EXPORT_HISTORY_EXPIRY_MS = EXPORT_HISTORY_EXPIRY_DAYS * 24 * 60 * 60 * 1000;

/** Section heading the refresh control sits beside. */
export const EXPORT_HISTORY_TITLE = "Previous exports";

/** Refresh control label while idle. */
export const EXPORT_HISTORY_REFRESH_IDLE = "Refresh status";

/** Refresh control label while a re-read is in flight. */
export const EXPORT_HISTORY_REFRESH_BUSY = "Refreshing";

/** Empty-view heading once the first response lands with no rows. */
export const EXPORT_HISTORY_EMPTY_TITLE = "No exports yet";

/** Empty-view explanation of where future exports appear. */
export const EXPORT_HISTORY_EMPTY_DESCRIPTION =
  "You don't have any export records right now. Once you export data, all records will appear here.";

/** Download cell text once the seven days have passed. */
export const EXPORT_HISTORY_EXPIRED_LABEL = "Expired";

/** Download cell placeholder for fresh rows that are not downloadable. */
export const EXPORT_HISTORY_DOWNLOAD_IDLE = "-";

/** Download cell link text for a fresh, finished export. */
export const EXPORT_HISTORY_DOWNLOAD_LABEL = "Download";

/** Loading announcement the skeleton block exposes while pending. */
export const EXPORT_HISTORY_LOADING_LABEL = "Loading export history";

/** Inline failure note; the header refresh doubles as the retry. */
export const EXPORT_HISTORY_ERROR_MESSAGE = "Couldn't load export history.";

/**
 * Whether the record's download has lapsed: `now` at or past created-at
 * plus seven days. The exact boundary reads expired.
 */
export function isExpired(createdAt: string, now: Date): boolean {
  return now.getTime() >= new Date(createdAt).getTime() + EXPORT_HISTORY_EXPIRY_MS;
}

/**
 * Whether the history must keep re-reading: any row still processing. A
 * queued row alone never starts polling; terminal rows settle it.
 */
export function shouldPoll(records: readonly { status: string }[]): boolean {
  return records.some((record) => record.status === "processing");
}

/** Polling fragment the history query spreads into its options. */
export interface ExportHistoryPoll {
  refetchInterval: number | false;
}

/** Poll while work runs; go idle once every row has settled. */
export function historyQuery(records: readonly { status: string }[]): ExportHistoryPoll {
  return { refetchInterval: shouldPoll(records) ? EXPORT_HISTORY_POLL_MS : false };
}

/** Visible format label; unknown providers render the unknown marker. */
export function historyFormatLabel(provider: string): string {
  if (provider === "csv") return "CSV";
  if (provider === "xlsx") return "Excel";
  if (provider === "json") return "JSON";
  return "Unknown";
}

/** Lifecycle states the status marker can show, plus the unknown marker. */
export type ExportHistoryStatus = "queued" | "processing" | "completed" | "failed" | "unknown";

/** Stored status as a marker state; unknown strings become the marker. */
export function historyStatus(status: string): ExportHistoryStatus {
  return isKnownExportStatus(status) ? status : "unknown";
}

/** Marker tone per state: waiting neutral, in-flight amber, done green, failed red. */
export const EXPORT_HISTORY_STATUS_TONES: Record<ExportHistoryStatus, BadgeTone> = {
  queued: "neutral",
  processing: "warning",
  completed: "success",
  failed: "danger",
  unknown: "neutral",
};

/** Who started the export, as the row's first cell shows them. */
export interface ExportHistoryInitiator {
  /** Display name, falling back to the joined first/last names. */
  name: string;
  /** Single fallback character when no avatar image exists. */
  initial: string;
  /** Avatar image URL, or null when the row shows the initial. */
  avatarUrl: string | null;
}

/**
 * Initiator facts from the user-lite payload (which carries no email):
 * the name prefers the display name, the initial the display name, then
 * the first name, then a question mark.
 */
export function historyInitiator(detail: WorkspaceExport["initiated_by_detail"]): ExportHistoryInitiator {
  const displayName = detail.display_name.trim();
  const firstName = detail.first_name.trim();
  const joinedNames = [firstName, detail.last_name.trim()].filter((part) => part !== "").join(" ");
  const initialSource = displayName !== "" ? displayName : firstName;
  return {
    name: displayName !== "" ? displayName : joinedNames,
    initial: initialSource === "" ? "?" : initialSource.charAt(0),
    avatarUrl: detail.avatar_url !== null && detail.avatar_url.trim() !== "" ? detail.avatar_url : null,
  };
}

const STARTED_FORMAT = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" });

/** Start date as the row shows it, e.g. `Apr 12, 2024`. */
export function historyStartedLabel(createdAt: string): string {
  return STARTED_FORMAT.format(new Date(createdAt));
}

/** Download cell shape: fresh-link, idle dash, or lapsed marker. */
export type ExportHistoryDownload = { kind: "link"; href: string } | { kind: "dash" } | { kind: "expired" };

/**
 * Download cell rule: lapsed rows lose the control whatever their state;
 * otherwise only a finished row with a file URL links out, and every
 * other fresh row shows the idle dash.
 */
export function historyDownload(
  record: Pick<WorkspaceExport, "created_at" | "status" | "url">,
  now: Date
): ExportHistoryDownload {
  if (isExpired(record.created_at, now)) return { kind: "expired" };
  if (record.status === "completed" && record.url !== null && record.url !== "") {
    return { kind: "link", href: record.url };
  }
  return { kind: "dash" };
}

/** Every fact one history row renders, left to right. */
export interface ExportHistoryRowFacts {
  initiatorName: string;
  initiatorInitial: string;
  avatarUrl: string | null;
  startedLabel: string;
  projectCount: number;
  projectLabel: string;
  formatLabel: string;
  status: ExportHistoryStatus;
  statusTone: BadgeTone;
  download: ExportHistoryDownload;
}

/**
 * Row facts for one record. A null project list counts as zero projects;
 * unknown providers and statuses render their unknown markers instead of
 * crashing the list.
 */
export function historyRowFacts(record: WorkspaceExport, options: { now: Date }): ExportHistoryRowFacts {
  const initiator = historyInitiator(record.initiated_by_detail);
  const status = historyStatus(record.status);
  const projectCount = record.project === null ? 0 : record.project.length;
  return {
    initiatorName: initiator.name,
    initiatorInitial: initiator.initial,
    avatarUrl: initiator.avatarUrl,
    startedLabel: historyStartedLabel(record.created_at),
    projectCount,
    projectLabel: `${projectCount} project(s)`,
    formatLabel: historyFormatLabel(record.provider),
    status,
    statusTone: EXPORT_HISTORY_STATUS_TONES[status],
    download: historyDownload(record, options.now),
  };
}
