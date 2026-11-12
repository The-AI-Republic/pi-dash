// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paid-plan free-quota reminder (NEWPIDASH-181). Free allowance exhaustion
// on payg/member plans is normally silent (the wallet takes over). The
// opt-in reminder fires at most once per UTC day, informational only,
// never blocking a turn, run, or composer. The setting itself persists
// server-side in Profile.settings; the shown-today marker is device-local
// (UTC-date key), so the setting follows the user while the note re-fires
// after each midnight UTC rollover.
import { normalizePlanId } from "@pidash/api-client";

/** localStorage key prefix for the shown-today markers. */
export const FREE_QUOTA_REMINDER_STORAGE_PREFIX = "pi-dash:free-quota-reminder:shown:";

/** Paid plans see the toggle; free plans keep the existing 402 notice instead. */
export function isPaidPlan(plan: unknown): boolean {
  const id = normalizePlanId(plan);
  return id === "payg" || id === "member";
}

/** Exhausted when usage reaches the limit; a zero/negative limit is not exhausted. */
export function isFreeAllowanceExhausted(used: number | null | undefined, limit: number | null | undefined): boolean {
  if (typeof used !== "number" || typeof limit !== "number") return false;
  if (!Number.isFinite(used) || !Number.isFinite(limit)) return false;
  if (limit <= 0) return false;
  return used >= limit;
}

/** The current UTC day as `YYYY-MM-DD`; the rollover key for once-per-day. */
export function utcDayKey(at: Date = new Date()): string {
  return at.toISOString().slice(0, 10);
}

/** Minimal storage surface, so tests can supply a fake without touching localStorage. */
export interface ReminderStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Read the platform storage when available (guarded for SSR/tests). */
export function platformStorage(): ReminderStorage | null {
  try {
    if (typeof globalThis === "undefined" || !("localStorage" in globalThis)) return null;
    const storage = globalThis.localStorage;
    if (storage === null || storage === undefined) return null;
    return storage;
  } catch {
    return null;
  }
}

/** Whether the reminder was already shown for this UTC day. */
export function hasShownToday(storage: ReminderStorage | null, dayKey: string): boolean {
  if (storage === null) return false;
  try {
    return storage.getItem(`${FREE_QUOTA_REMINDER_STORAGE_PREFIX}${dayKey}`) !== null;
  } catch {
    return false;
  }
}

/** Record that the reminder has been shown for this UTC day. */
export function markShownToday(storage: ReminderStorage | null, dayKey: string): void {
  if (storage === null) return;
  try {
    storage.setItem(`${FREE_QUOTA_REMINDER_STORAGE_PREFIX}${dayKey}`, "1");
  } catch {
    // A blocked store degrades to "shown every visit" rather than a throw
    // inside a render; the setting and quota reads are unaffected.
  }
}

/**
 * The full gate: show the note only when the toggle is on, the plan is
 * paid, the allowance is exhausted, and it has not already shown today.
 */
export function shouldShowFreeQuotaReminder(args: {
  enabled: boolean;
  plan: string | null | undefined;
  used: number | null | undefined;
  limit: number | null | undefined;
  storage: ReminderStorage | null;
  dayKey: string;
}): boolean {
  if (!args.enabled) return false;
  if (!isPaidPlan(args.plan)) return false;
  if (!isFreeAllowanceExhausted(args.used, args.limit)) return false;
  if (hasShownToday(args.storage, args.dayKey)) return false;
  return true;
}
