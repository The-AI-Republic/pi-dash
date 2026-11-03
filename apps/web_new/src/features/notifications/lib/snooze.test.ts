// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Snooze rules (parity/specs/notifications.md §9, NTF-020-022): the five
// preset delays, the custom picker's day/slot composition (noon and
// midnight included, per the NEWFRONT-213 target), elapsed-slot hiding,
// and the resume label shape.
import { describe, expect, it } from "vitest";

import {
  SLOTS_PER_MERIDIEM,
  SNOOZE_PRESETS,
  composeResumeInstant,
  dayIsFullyElapsed,
  earliestSnoozeDay,
  formatResumeLabel,
  offeredSlots,
  pickedDayLabel,
  presetResumeInstant,
  slotLabels,
  slotTo24Hour,
  slotToMinutes,
  snoozePresetByLabel,
} from "./snooze.js";

const NOON = new Date(2026, 9, 17, 12, 0, 0, 0).getTime();
const DAY_MS = 24 * 60 * 60 * 1000;

describe("SNOOZE_PRESETS", () => {
  it("offers exactly the five fixed delays in menu order", () => {
    expect(SNOOZE_PRESETS.map((preset) => preset.label)).toEqual(["1 day", "3 days", "5 days", "1 week", "2 weeks"]);
    expect(SNOOZE_PRESETS.map((preset) => preset.delayMs)).toEqual([
      DAY_MS,
      3 * DAY_MS,
      5 * DAY_MS,
      7 * DAY_MS,
      14 * DAY_MS,
    ]);
  });

  it("lands each preset that far from the moment of snoozing", () => {
    for (const preset of SNOOZE_PRESETS) {
      expect(presetResumeInstant(preset, NOON).getTime()).toBe(NOON + preset.delayMs);
    }
  });

  it("looks presets up by exact menu wording only", () => {
    expect(snoozePresetByLabel("1 week")?.id).toBe("week-1");
    expect(snoozePresetByLabel("1 Week")).toBeNull();
    expect(snoozePresetByLabel("2 days")).toBeNull();
  });
});

describe("slot grid", () => {
  it("offers 24 half-hour slots per meridiem from 12:00 down", () => {
    const labels = slotLabels();
    expect(labels).toHaveLength(SLOTS_PER_MERIDIEM);
    expect(labels[0]).toBe("12:00");
    expect(labels[1]).toBe("12:30");
    expect(labels[2]).toBe("01:00");
    expect(labels[labels.length - 1]).toBe("11:30");
    for (const label of labels) expect(label).toMatch(/^(0[1-9]|1[0-2]):(00|30)$/);
  });

  it("composes 12 AM to midnight and 12 PM to noon (NEWFRONT-213 target)", () => {
    expect(slotTo24Hour("12", "AM")).toBe(0);
    expect(slotTo24Hour("12", "PM")).toBe(12);
  });

  it("shifts other twelve-hour picks under PM", () => {
    expect(slotTo24Hour("01", "AM")).toBe(1);
    expect(slotTo24Hour("01", "PM")).toBe(13);
    expect(slotTo24Hour("11", "PM")).toBe(23);
  });

  it("counts minutes past midnight", () => {
    expect(slotToMinutes("10:30", "AM")).toBe(10 * 60 + 30);
    expect(slotToMinutes("12:30", "PM")).toBe(12 * 60 + 30);
    expect(slotToMinutes("12:30", "AM")).toBe(30);
  });

  it("reads a slot without minutes as a whole hour", () => {
    expect(slotToMinutes("10", "PM")).toBe(22 * 60);
  });
});

describe("composeResumeInstant", () => {
  it("needs both a day and a slot", () => {
    expect(composeResumeInstant(null, "10:30", "AM")).toBeNull();
    expect(composeResumeInstant("2026-10-19", null, "AM")).toBeNull();
  });

  it("builds the local instant for the picked day and slot", () => {
    expect(composeResumeInstant("2026-10-19", "10:30", "AM")?.getTime()).toBe(
      new Date(2026, 9, 19, 10, 30, 0, 0).getTime()
    );
  });

  it("builds noon for a 12 PM pick and midnight for 12 AM", () => {
    expect(composeResumeInstant("2026-10-19", "12:30", "PM")?.getTime()).toBe(
      new Date(2026, 9, 19, 12, 30, 0, 0).getTime()
    );
    expect(composeResumeInstant("2026-10-19", "12:30", "AM")?.getTime()).toBe(
      new Date(2026, 9, 19, 0, 30, 0, 0).getTime()
    );
  });

  it("returns null for a malformed slot", () => {
    expect(composeResumeInstant("2026-10-19", "lunch", "AM")).toBeNull();
  });
});

describe("offeredSlots", () => {
  it("offers the full grid on a future day", () => {
    expect(offeredSlots("2026-10-19", NOON, "AM")).toEqual(slotLabels());
    expect(offeredSlots("2026-10-19", NOON, "PM")).toEqual(slotLabels());
  });

  it("offers the full grid on a past day only in the server's terms", () => {
    expect(offeredSlots("2026-10-15", NOON, "AM")).toEqual([]);
  });

  it("hides today's morning slots once noon has passed", () => {
    const offered = offeredSlots("2026-10-17", NOON, "AM");
    expect(offered).toEqual([]);
  });

  it("keeps every morning slot on a day that starts before them", () => {
    const earlyMorning = new Date(2026, 9, 17, 0, 20, 0, 0).getTime();
    const offered = offeredSlots("2026-10-17", earlyMorning, "AM");
    expect(offered).not.toContain("12:00");
    expect(offered[0]).toBe("12:30");
    expect(offered[offered.length - 1]).toBe("11:30");
    expect(offered).toHaveLength(23);
    for (const slot of offered) expect(slot).toMatch(/^\d{2}:(00|30)$/);
  });

  it("hides the whole morning and the current afternoon slot at 12:30", () => {
    const atHalfPastNoon = new Date(2026, 9, 17, 12, 30, 0, 0).getTime();
    expect(offeredSlots("2026-10-17", atHalfPastNoon, "AM")).toEqual([]);
    const afternoon = offeredSlots("2026-10-17", atHalfPastNoon, "PM");
    expect(afternoon).not.toContain("12:30");
    expect(afternoon).toHaveLength(22);
  });

  it("reports a same-day evening as fully elapsed", () => {
    const evening = new Date(2026, 9, 17, 23, 45, 0, 0).getTime();
    expect(dayIsFullyElapsed("2026-10-17", evening)).toBe(true);
    expect(offeredSlots("2026-10-17", evening, "AM")).toEqual([]);
    expect(offeredSlots("2026-10-17", evening, "PM")).toEqual([]);
  });

  it("never reports a future day as elapsed", () => {
    expect(dayIsFullyElapsed("2026-10-18", NOON)).toBe(false);
  });
});

describe("day helpers", () => {
  it("starts the picker at local today", () => {
    expect(earliestSnoozeDay(NOON)).toBe("2026-10-17");
  });

  it("labels a picked day for the trigger", () => {
    expect(pickedDayLabel("2026-10-19")).toBe("Oct 19, 2026");
    expect(pickedDayLabel("2027-01-02")).toBe("Jan 2, 2027");
    expect(pickedDayLabel("not-a-day")).toBe("not-a-day");
  });
});

describe("formatResumeLabel", () => {
  it("reads the resumed moment in the oracle's pinned shape", () => {
    expect(formatResumeLabel(new Date(2026, 9, 12, 10, 30).toISOString())).toBe("Till Oct 12, 2026, 10:30 AM");
  });

  it("pads the day and hour and picks the right meridiem", () => {
    expect(formatResumeLabel(new Date(2027, 0, 5, 1, 5).toISOString())).toBe("Till Jan 05, 2027, 01:05 AM");
    expect(formatResumeLabel(new Date(2026, 11, 31, 23, 59).toISOString())).toBe("Till Dec 31, 2026, 11:59 PM");
  });

  it("renders noon as 12 PM and midnight as 12 AM", () => {
    expect(formatResumeLabel(new Date(2026, 9, 19, 12, 30).toISOString())).toBe("Till Oct 19, 2026, 12:30 PM");
    expect(formatResumeLabel(new Date(2026, 9, 19, 0, 30).toISOString())).toBe("Till Oct 19, 2026, 12:30 AM");
  });

  it("returns an empty string for an unparseable stamp", () => {
    expect(formatResumeLabel("whenever")).toBe("");
  });
});
