// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snooze scheduling (written from parity/specs/notifications.md §9,
// NTF-020/021/022). Three pieces: the fixed preset delays, the custom
// resume picker's day-and-slot rules, and the resume label a snoozed card
// shows in place of its age. Pure functions over local time; no React and
// no network, so every rule is unit-testable at a fixed instant.
import { MONTH_ABBREVS, todayIsoDay } from "../../issues/index.js";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** Every hour of the day carries two half-hour slots (24 AM + 24 PM). */
export const SLOTS_PER_MERIDIEM = 24;

/** A snooze preset: what the menu says and how far out it lands. */
export interface SnoozePreset {
  /** Stable identifier; the menu key and the test hook suffix. */
  id: "day-1" | "day-3" | "day-5" | "week-1" | "week-2";
  /** Menu wording, e.g. "3 days". */
  label: string;
  /** Delay from the moment of snoozing. */
  delayMs: number;
}

/** The five preset delays, shortest first. */
export const SNOOZE_PRESETS: readonly SnoozePreset[] = [
  { id: "day-1", label: "1 day", delayMs: DAY_MS },
  { id: "day-3", label: "3 days", delayMs: 3 * DAY_MS },
  { id: "day-5", label: "5 days", delayMs: 5 * DAY_MS },
  { id: "week-1", label: "1 week", delayMs: 7 * DAY_MS },
  { id: "week-2", label: "2 weeks", delayMs: 14 * DAY_MS },
];

/** Look a preset up by its menu wording (exact match), or null. */
export function snoozePresetByLabel(label: string): SnoozePreset | null {
  return SNOOZE_PRESETS.find((preset) => preset.label === label) ?? null;
}

/** The instant a preset lands on, measured from `nowMs`. */
export function presetResumeInstant(preset: SnoozePreset, nowMs: number): Date {
  return new Date(nowMs + preset.delayMs);
}

/**
 * Slot icon text for a two-digit hour and the meridiem it was picked
 * under. Picks are made in twelve-hour form, so the 12 slot is midnight
 * under AM and noon under PM (the NEWFRONT-213 target), and any other
 * hour shifts by twelve under PM.
 */
export function slotTo24Hour(slot: string, meridiem: "AM" | "PM"): number {
  const twelve = Number(slot);
  const base = twelve % 12;
  return meridiem === "PM" ? base + 12 : base;
}

/**
 * The half-hour slot labels for one meridiem: "01:00" through "12:30".
 * `slotTo24Hour` and `slotToMinutes` turn them into an instant.
 */
export function slotLabels(): string[] {
  const labels: string[] = [];
  for (let index = 0; index < SLOTS_PER_MERIDIEM; index++) {
    const hour = Math.floor(index / 2) % 12 === 0 ? 12 : Math.floor(index / 2) % 12;
    const minute = index % 2 === 0 ? "00" : "30";
    labels.push(`${String(hour).padStart(2, "0")}:${minute}`);
  }
  return labels;
}

/** Minutes past local midnight for one twelve-hour slot. */
export function slotToMinutes(slot: string, meridiem: "AM" | "PM"): number {
  const [hourText = "", minuteText = ""] = slot.split(":");
  const hours = slotTo24Hour(hourText, meridiem);
  return hours * 60 + Number(minuteText);
}

/**
 * The local instant a day plus a twelve-hour slot means. Both parts are
 * required; null means "not enough picked yet" rather than an error.
 */
export function composeResumeInstant(day: string | null, slot: string | null, meridiem: "AM" | "PM"): Date | null {
  if (day === null || slot === null) return null;
  const minutes = slotToMinutes(slot, meridiem);
  if (Number.isNaN(minutes)) return null;
  const [year, month, date] = day.split("-").map(Number) as [number, number, number];
  return new Date(year, month - 1, date, Math.floor(minutes / 60), minutes % 60, 0, 0);
}

/**
 * Slot labels still ahead of `nowMs` on `day`: every slot on a later day,
 * only the strictly future ones today, none on an earlier day. Slots
 * equal to the current minute count as elapsed, matching "already
 * elapsed" rather than "elapsed this second".
 */
export function offeredSlots(day: string, nowMs: number, meridiem: "AM" | "PM"): string[] {
  const now = new Date(nowMs);
  const today = todayIsoDay(now);
  if (day > today) return slotLabels();
  if (day < today) return [];
  const nowMinutes = now.getHours() * 60 + now.getMinutes();
  return slotLabels().filter((slot) => slotToMinutes(slot, meridiem) > nowMinutes);
}

/** True when today has no slot left in either meridiem. */
export function dayIsFullyElapsed(day: string, nowMs: number): boolean {
  return (
    day <= todayIsoDay(new Date(nowMs)) &&
    offeredSlots(day, nowMs, "AM").length === 0 &&
    offeredSlots(day, nowMs, "PM").length === 0
  );
}

/** The earliest day the picker may offer: the member's local today. */
export function earliestSnoozeDay(nowMs: number): string {
  return todayIsoDay(new Date(nowMs));
}

/**
 * The resume label a snoozed card shows instead of its age:
 * "Till Oct 12, 2026, 10:30 AM" — abbreviated month, zero-padded day and
 * twelve-hour clock, matching the oracle's pinned shape.
 */
export function formatResumeLabel(snoozedTill: string): string {
  const parsed = new Date(snoozedTill);
  if (Number.isNaN(parsed.getTime())) return "";
  const period = parsed.getHours() >= 12 ? "PM" : "AM";
  const twelve = parsed.getHours() % 12 === 0 ? 12 : parsed.getHours() % 12;
  const time = `${String(twelve).padStart(2, "0")}:${String(parsed.getMinutes()).padStart(2, "0")} ${period}`;
  const month = MONTH_ABBREVS[parsed.getMonth()];
  const day = String(parsed.getDate()).padStart(2, "0");
  return `Till ${month} ${day}, ${parsed.getFullYear()}, ${time}`;
}

/** Short label for the dialog's date trigger: "Oct 12, 2026". */
export function pickedDayLabel(day: string): string {
  const [year, month, date] = day.split("-").map(Number) as [number, number, number];
  const monthName = MONTH_ABBREVS[month - 1];
  if (monthName === undefined || Number.isNaN(date) || Number.isNaN(year)) return day;
  return `${monthName} ${date}, ${year}`;
}
