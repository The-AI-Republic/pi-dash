// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Triage helpers (parity/specs/intake.md INT-020–023, 025). Pure: the
// overflow entries around the header's copy item, the status chip's
// text and tone (including the snooze countdown and the lapsed passed
// styling), the snooze day math, and the post-action focus landing.
// The short entry labels match the oracle pins in
// e2e/parity/intake/triage.spec.ts, which the shared scenarios assert
// verbatim against both apps; the dialog bodies stay original.

import { INTAKE_STATUS } from "@pidash/api-client";
import type { MenuEntry } from "@pidash/kit";

/** Overflow entry opening the snooze dialog on a pending request. */
export const TRIAGE_SNOOZE_LABEL = "Snooze";
/** Overflow entry clearing the snooze on a snoozed request. */
export const TRIAGE_UNSNOOZE_LABEL = "Un snooze";
/** Overflow entry opening the duplicate-target picker. */
export const TRIAGE_DUPLICATE_LABEL = "Mark as duplicate";
/** Overflow entry opening the delete confirmation. */
export const TRIAGE_DELETE_LABEL = "Delete";

/** Notice title when a member clicks an admin-only triage action. */
export const TRIAGE_REFUSED_TITLE = "Permission denied";

export type TriageRefusedAction = "accept" | "decline" | "snooze" | "duplicate";

/**
 * Notice body naming the admin requirement for one refused action.
 * Un-snooze shares the snooze sentence.
 */
export function triageRefusedMessage(action: TriageRefusedAction): string {
  switch (action) {
    case "accept":
      return "Only a project admin can accept requests.";
    case "decline":
      return "Only a project admin can decline requests.";
    case "snooze":
      return "Only a project admin can snooze requests.";
    case "duplicate":
      return "Only a project admin can mark requests as duplicate.";
  }
}

export interface TriageMenuHandlers {
  /** The request is snoozed, so the entry clears instead of opening. */
  snoozed: boolean;
  /** Admin or creator standing on an open request. */
  showDelete: boolean;
  onSnooze: () => void;
  onUnsnooze: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}

export interface TriageMenuEntries {
  /** Entries ahead of the header's copy-link item. */
  lead: MenuEntry[];
  /** Entries behind it. */
  trail: MenuEntry[];
}

/**
 * The triage overflow entries split around the copy-link item the
 * header owns: snooze (or un-snooze) and mark-as-duplicate lead, and
 * delete trails for viewers who may remove the request.
 */
export function buildTriageMenuEntries(handlers: TriageMenuHandlers): TriageMenuEntries {
  const lead: MenuEntry[] = [
    handlers.snoozed
      ? { label: TRIAGE_UNSNOOZE_LABEL, onSelect: handlers.onUnsnooze }
      : { label: TRIAGE_SNOOZE_LABEL, onSelect: handlers.onSnooze },
    { label: TRIAGE_DUPLICATE_LABEL, onSelect: handlers.onDuplicate },
  ];
  const trail: MenuEntry[] = handlers.showDelete ? [{ label: TRIAGE_DELETE_LABEL, onSelect: handlers.onDelete }] : [];
  return { lead, trail };
}

/** Local ISO day of an ISO timestamp; null when absent or unparseable. */
export function snoozeDayFromTimestamp(value: string | null): string | null {
  if (value === null) return null;
  const time = Date.parse(value);
  if (Number.isNaN(time)) return null;
  const at = new Date(time);
  const month = String(at.getMonth() + 1).padStart(2, "0");
  const day = String(at.getDate()).padStart(2, "0");
  return `${at.getFullYear()}-${month}-${day}`;
}

/**
 * Whole calendar days from a local today to a snooze day; null when
 * either side is absent or malformed. Negative once the day has passed.
 */
export function snoozeDaysLeft(snoozedDay: string | null, todayDay: string): number | null {
  const snoozed = parseDay(snoozedDay);
  const today = parseDay(todayDay);
  if (snoozed === null || today === null) return null;
  return Math.round((snoozed - today) / 86_400_000);
}

function parseDay(value: string | null): number | null {
  if (value === null) return null;
  const parts = value.split("-");
  if (parts.length !== 3 || parts.some((part) => part === "" || !Number.isInteger(Number(part)))) return null;
  const [year, month, day] = parts.map(Number) as [number, number, number];
  const time = Date.UTC(year, month - 1, day);
  const check = new Date(time);
  if (check.getUTCFullYear() !== year || check.getUTCMonth() !== month - 1 || check.getUTCDate() !== day) return null;
  return time;
}

/**
 * The instant a snooze day stores: that day's local midnight as an ISO
 * timestamp. Null when the day is malformed.
 */
export function snoozeTimestampForDay(isoDay: string): string | null {
  const parts = isoDay.split("-");
  if (parts.length !== 3 || parts.some((part) => part === "" || !Number.isInteger(Number(part)))) return null;
  const [year, month, day] = parts.map(Number) as [number, number, number];
  const at = new Date(year, month - 1, day);
  if (at.getFullYear() !== year || at.getMonth() !== month - 1 || at.getDate() !== day) return null;
  return at.toISOString();
}

export type IntakeStatusTone = "default" | "danger";

export interface IntakeStatusChipModel {
  text: string;
  tone: IntakeStatusTone;
}

/**
 * The detail's status chip (INT-022/024): the resolved names, Pending,
 * and the snooze countdown in whole days. A lapsed snooze keeps the
 * chip with the passed wording and danger tone (NEWFRONT-272), and an
 * unknown status reads as pending rather than vanishing.
 */
export function selectIntakeStatusChip(
  status: number,
  snoozedTill: string | null,
  todayIso: string
): IntakeStatusChipModel {
  if (status === INTAKE_STATUS.accepted) return { text: "Accepted", tone: "default" };
  if (status === INTAKE_STATUS.declined) return { text: "Declined", tone: "default" };
  if (status === INTAKE_STATUS.duplicate) return { text: "Duplicate", tone: "default" };
  if (status === INTAKE_STATUS.snoozed) {
    const left = snoozeDaysLeft(snoozeDayFromTimestamp(snoozedTill), todayIso);
    if (left === null) return { text: "Snoozed", tone: "default" };
    if (left < 0) return { text: "Overdue", tone: "danger" };
    if (left === 0) return { text: "Due today", tone: "default" };
    if (left === 1) return { text: "1 day to go", tone: "default" };
    return { text: `${left} days to go`, tone: "default" };
  }
  return { text: "Pending", tone: "default" };
}

/**
 * Move focus once the address's new selection has painted: the landing
 * may not exist on the first frame after a triage advance, so retry
 * across frames and give up quietly when it never arrives.
 */
export function scheduleTriageFocus(emptied: boolean, attempts = 10): void {
  requestAnimationFrame(() => {
    if (focusTriageLanding(emptied)) return;
    if (attempts > 1) scheduleTriageFocus(emptied, attempts - 1);
  });
}

/**
 * Move focus after a triage action (INT-020–025 accessibility): the
 * neighbour's detail heading — the editable title field when the
 * request can be edited, else the detail container — or, when the tab
 * emptied, the list's first row with the list pane as the fallback.
 * True when something took focus.
 */
export function focusTriageLanding(emptied: boolean, root: Document | Element = document): boolean {
  if (!emptied) {
    const detail = root.querySelector('[data-testid="intake-detail"]');
    if (detail === null) return false;
    // The shared title field names its editable box "Title"
    // (IssueTitleField.TITLE_LABEL); the read-only heading is static
    // text, so the focusable detail container takes focus instead.
    const target = detail.querySelector('textarea[aria-label="Title"]') ?? detail;
    (target as HTMLElement).focus();
    return true;
  }
  const row = root.querySelector('[data-testid="intake-row"]');
  if (row !== null) {
    (row as HTMLElement).focus();
    return true;
  }
  const pane = root.querySelector('[data-testid="intake-list-pane"]');
  if (pane === null) return false;
  (pane as HTMLElement).focus();
  return true;
}
