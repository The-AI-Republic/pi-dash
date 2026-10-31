// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal-access-token presentation rules (parity/specs/workspace-settings.md
// §§25-28, WSET-025-028). Pure functions shared by the page, dialogs and
// tests: title validation, expiry presets, the live expiry line, row
// liveness/wording, and the one-time CSV backup. No React, no network.

import type { PersonalApiToken } from "@pidash/api-client";

export const TOKEN_HEADING = "Personal Access Tokens";
export const TOKEN_ADD_LABEL = "Add personal access token";
export const TOKEN_EMPTY_TITLE = "No Personal token yet";
export const TOKEN_EMPTY_ACTION = "Add API token";
export const TOKEN_EMPTY_DESCRIPTION = "Create a token to connect scripts, integrations or agents to this account.";
export const TOKEN_CREATE_TITLE = "Generate a personal access token";
export const TOKEN_CREATE_DESCRIPTION =
  "Name the token and choose when it stops working. You will see its secret only once.";
export const TOKEN_REVEAL_TITLE = "Key created";
export const TOKEN_DELETE_TITLE = "Delete personal access token";
export const TOKEN_LOADING_LABEL = "Loading personal access tokens";
export const TOKEN_LOAD_FAILED = "Personal access tokens could not be loaded.";

export const TITLE_REQUIRED_ERROR = "Title is required";
export const TITLE_TOO_LONG_ERROR = "Title should be less than 255 characters";
export const EXPIRY_REQUIRED_NOTICE = "Please select an expiration date";
export const TOKEN_COPIED_NOTICE = "Token copied to clipboard";
export const TOKEN_DELETED_NOTICE = "Personal access token successfully deleted";
export const TOKEN_CREATE_FAILED_TITLE = "Could not create the personal access token";
export const TOKEN_DELETE_FAILED_TITLE = "Could not delete the personal access token";
export const TOKEN_BACKUP_FAILED_NOTICE = "The backup file could not be downloaded";

/** Longest accepted title; the oracle rejects a 256-character title. */
export const TOKEN_TITLE_MAX = 255;

export type ExpiryPresetKey = "week" | "month" | "quarter" | "year" | "custom";

export interface ExpiryPreset {
  key: ExpiryPresetKey;
  label: string;
}

/** The select order the create dialog renders. */
export const EXPIRY_PRESETS: readonly ExpiryPreset[] = [
  { key: "week", label: "1 week" },
  { key: "month", label: "1 month" },
  { key: "quarter", label: "3 months" },
  { key: "year", label: "1 year" },
  { key: "custom", label: "Custom" },
];

/** Nothing picked yet; the picker shows its placeholder. */
export type ExpiryChoice = ExpiryPresetKey | null;

export interface TokenDraft {
  title: string;
  description: string;
  choice: ExpiryChoice;
  customDay: string | null;
  neverExpires: boolean;
}

export function emptyTokenDraft(): TokenDraft {
  return { title: "", description: "", choice: null, customDay: null, neverExpires: false };
}

/** Inline title error, or null while the value is acceptable. */
export function titleError(title: string): string | null {
  const trimmed = title.trim();
  if (trimmed === "") return TITLE_REQUIRED_ERROR;
  if (trimmed.length > TOKEN_TITLE_MAX) return TITLE_TOO_LONG_ERROR;
  return null;
}

/** Local ISO day for a date, in the machine's own calendar. */
export function localIsoDay(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

/** The earliest custom day a token may expire, one local day ahead. */
export function earliestCustomDay(now: Date = new Date()): string {
  const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return localIsoDay(next);
}

function addMonths(date: Date, months: number): Date {
  const result = new Date(date.getTime());
  // A preset adds calendar months and clamps to the last day of the
  // target month, the way month arithmetic reads: Jan 31 plus one month
  // is Feb 28/29, never Mar 2/3 (parity: the old app adds months, it
  // does not roll into the following month).
  const day = result.getDate();
  result.setMonth(result.getMonth() + months);
  if (result.getDate() !== day) result.setDate(0);
  return result;
}

/**
 * Resolve the draft's expiry to an ISO timestamp, or null when it never
 * expires. A custom day keeps the current time of day; presets count
 * forward from now. Returns undefined while a required date is missing so
 * the caller can show the missing-date notice instead of submitting.
 */
export function resolveExpiry(draft: TokenDraft, now: Date = new Date()): string | null | undefined {
  if (draft.neverExpires) return null;
  switch (draft.choice) {
    case "week":
      return new Date(now.getTime() + 7 * 24 * 60 * 60 * 1000).toISOString();
    case "month":
      return addMonths(now, 1).toISOString();
    case "quarter":
      return addMonths(now, 3).toISOString();
    case "year":
      return addMonths(now, 12).toISOString();
    case "custom": {
      if (draft.customDay === null) return undefined;
      const [year, month, day] = draft.customDay.split("-").map(Number) as [number, number, number];
      const combined = new Date(
        year,
        month - 1,
        day,
        now.getHours(),
        now.getMinutes(),
        now.getSeconds(),
        now.getMilliseconds()
      );
      return combined.toISOString();
    }
    case null:
      return undefined;
  }
}

/**
 * `Expires Jun 15, 2030 at 12:00` — the wording rows, the reveal and the
 * preview use. Rendered in the member's timezone (UTC until the member
 * picks one), never the browser's: the stored instant is absolute, but
 * the day and clock the member reads follow their own zone.
 */
export function expiryLine(expiredAt: string | null | undefined, timeZone = "UTC"): string {
  if (expiredAt === null || expiredAt === undefined) return "Never expires";
  const date = new Date(expiredAt);
  if (Number.isNaN(date.getTime())) return "Never expires";
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone,
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(date);
  const pick = (type: Intl.DateTimeFormatPartTypes): string => parts.find((part) => part.type === type)?.value ?? "";
  return `Expires ${pick("month")} ${pick("day")}, ${pick("year")} at ${pick("hour")}:${pick("minute")}`;
}

function pluralize(count: number, unit: string): string {
  return `${count} ${unit}${count === 1 ? "" : "s"}`;
}

/** Relative wording for an already-lapsed expiry (`Expired 2 days ago`). */
export function expiredAgoLine(expiredAt: string, now: Date = new Date()): string {
  const then = new Date(expiredAt).getTime();
  const diff = Math.max(0, now.getTime() - then);
  const minutes = Math.floor(diff / 60_000);
  const hours = Math.floor(diff / 3_600_000);
  const days = Math.floor(diff / 86_400_000);
  if (minutes < 1) return "Expired just now";
  if (hours < 1) return `Expired ${pluralize(minutes, "minute")} ago`;
  if (days < 1) return `Expired ${pluralize(hours, "hour")} ago`;
  if (days < 30) return `Expired ${pluralize(days, "day")} ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `Expired ${pluralize(months, "month")} ago`;
  return `Expired ${pluralize(Math.floor(months / 12), "year")} ago`;
}

/** The row's human expiry line: future, never, or relative past wording. */
export function rowExpiryLine(
  token: Pick<PersonalApiToken, "is_active" | "expired_at">,
  now: Date = new Date(),
  timeZone = "UTC"
): string {
  if (token.expired_at === null) return "Never expires";
  if (!token.is_active) return expiredAgoLine(token.expired_at, now);
  return expiryLine(token.expired_at, timeZone);
}

/** True when the server marked the row inactive (expired). */
export function isExpiredToken(token: Pick<PersonalApiToken, "is_active">): boolean {
  return !token.is_active;
}

/** The reveal's expiry line, for the freshly-created token's persisted value. */
export function revealExpiryLine(expiredAt: string | null, timeZone = "UTC"): string {
  return expiryLine(expiredAt, timeZone);
}

/** Empty descriptions do not render as a row paragraph. */
export function rowDescription(token: Pick<PersonalApiToken, "description">): string | null {
  const text = token.description.trim();
  return text === "" ? null : token.description;
}

export function backupFileName(now: Date = new Date()): string {
  return `secret-key-${now.getTime()}.csv`;
}

function csvCell(value: string): string {
  if (!/[",\n\r]/.test(value)) return value;
  return `"${value.replace(/"/g, '""')}"`;
}

/** The backup file: header, then the token's metadata and raw secret. */
export function tokenBackupCsv(input: { title: string; description: string; expiry: string; secret: string }): string {
  const title = input.title.trim();
  const header = "Title,Description,Expiry,Secret key";
  const row = [title, input.description, input.expiry, input.secret].map(csvCell).join(",");
  return `${header}\n${row}\n`;
}
