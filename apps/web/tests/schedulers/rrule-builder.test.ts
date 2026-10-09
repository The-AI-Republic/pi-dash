/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import type { RecurrenceDraft } from "@/components/project/scheduler-bindings/rrule-builder";
import { defaultDraft, parseRrule, serializeRrule } from "@/components/project/scheduler-bindings/rrule-builder";

// Monday, January 5 2026, 09:00 local wall time (the form's datetime-local shape).
const MONDAY = "2026-01-05T09:00";

function draft(patch: Partial<RecurrenceDraft>): RecurrenceDraft {
  return { ...defaultDraft(MONDAY), ...patch };
}

describe("serializeRrule — options → string", () => {
  it("daily default", () => {
    expect(serializeRrule(draft({ unit: "DAILY" }))).toBe("FREQ=DAILY");
  });

  it("omits INTERVAL=1 and emits INTERVAL=N", () => {
    expect(serializeRrule(draft({ unit: "DAILY", interval: 1 }))).toBe("FREQ=DAILY");
    expect(serializeRrule(draft({ unit: "DAILY", interval: 3 }))).toBe("FREQ=DAILY;INTERVAL=3");
  });

  it("guards INTERVAL >= 1 (the server 400s on 0)", () => {
    expect(serializeRrule(draft({ unit: "DAILY", interval: 0 }))).toBe("FREQ=DAILY");
    expect(serializeRrule(draft({ unit: "DAILY", interval: -2 }))).toBe("FREQ=DAILY");
  });

  it("weekly with weekday chips, sorted MO..SU", () => {
    expect(serializeRrule(draft({ unit: "WEEKLY", interval: 2, weekdays: [4, 0, 2] }))).toBe(
      "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR"
    );
  });

  it("monthly by day-of-month", () => {
    expect(serializeRrule(draft({ unit: "MONTHLY", monthlyMode: "monthday", monthday: 15 }))).toBe(
      "FREQ=MONTHLY;BYMONTHDAY=15"
    );
  });

  it("monthly by nth weekday, including last", () => {
    expect(serializeRrule(draft({ unit: "MONTHLY", monthlyMode: "weekday", nth: 3, nthWeekday: 1 }))).toBe(
      "FREQ=MONTHLY;BYDAY=3TU"
    );
    expect(serializeRrule(draft({ unit: "MONTHLY", monthlyMode: "weekday", nth: -1, nthWeekday: 4 }))).toBe(
      "FREQ=MONTHLY;BYDAY=-1FR"
    );
  });

  it("yearly", () => {
    expect(serializeRrule(draft({ unit: "YEARLY" }))).toBe("FREQ=YEARLY");
  });

  it("ends: UNTIL serialized as inclusive end-of-day UTC", () => {
    expect(serializeRrule(draft({ unit: "DAILY", ends: "until", untilDate: "2026-12-26" }))).toBe(
      "FREQ=DAILY;UNTIL=20261226T235959Z"
    );
  });

  it("ends: COUNT, clamped to >= 1", () => {
    expect(serializeRrule(draft({ unit: "WEEKLY", weekdays: [0], ends: "count", count: 13 }))).toBe(
      "FREQ=WEEKLY;BYDAY=MO;COUNT=13"
    );
    expect(serializeRrule(draft({ unit: "DAILY", ends: "count", count: 0 }))).toBe("FREQ=DAILY;COUNT=1");
  });

  it("single-shot (does not repeat) serializes to the empty string", () => {
    expect(serializeRrule(draft({ unit: "NONE" }))).toBe("");
  });

  it("never emits an RRULE: prefix or a DTSTART", () => {
    const s = serializeRrule(draft({ unit: "WEEKLY", ends: "until", untilDate: "2027-01-01" }));
    expect(s.startsWith("FREQ=")).toBe(true);
    expect(s).not.toContain("DTSTART");
  });
});

describe("parseRrule — string → options round-trip", () => {
  const roundTrips = [
    "FREQ=DAILY",
    "FREQ=DAILY;INTERVAL=3",
    "FREQ=WEEKLY;BYDAY=MO,WE,FR",
    "FREQ=WEEKLY;INTERVAL=2;BYDAY=SA,SU",
    "FREQ=MONTHLY;BYMONTHDAY=15",
    "FREQ=MONTHLY;BYDAY=3TU",
    "FREQ=MONTHLY;BYDAY=-1FR",
    "FREQ=YEARLY",
    "FREQ=DAILY;UNTIL=20261226T235959Z",
    "FREQ=WEEKLY;BYDAY=MO;COUNT=13",
    "",
  ];

  it.each(roundTrips)("round-trips %j", (rule) => {
    const parsed = parseRrule(rule, MONDAY);
    expect(parsed).not.toBeNull();
    expect(serializeRrule(parsed!)).toBe(rule);
  });

  it("parses weekly chips into rrule weekday numbers", () => {
    const parsed = parseRrule("FREQ=WEEKLY;BYDAY=MO,WE,FR", MONDAY);
    expect(parsed?.unit).toBe("WEEKLY");
    expect(parsed?.weekdays).toEqual([0, 2, 4]);
  });

  it("parses ends variants", () => {
    expect(parseRrule("FREQ=DAILY;COUNT=5", MONDAY)).toMatchObject({ ends: "count", count: 5 });
    expect(parseRrule("FREQ=DAILY;UNTIL=20261226T235959Z", MONDAY)).toMatchObject({
      ends: "until",
      untilDate: "2026-12-26",
    });
    expect(parseRrule("FREQ=DAILY", MONDAY)).toMatchObject({ ends: "never" });
  });

  it("tolerates a leading RRULE: prefix", () => {
    expect(parseRrule("RRULE:FREQ=DAILY", MONDAY)?.unit).toBe("DAILY");
  });

  it("empty string is the single-shot state", () => {
    expect(parseRrule("", MONDAY)?.unit).toBe("NONE");
    expect(parseRrule("   ", MONDAY)?.unit).toBe("NONE");
  });

  it("plain FREQ=WEEKLY defaults the chip to dtstart's weekday", () => {
    const parsed = parseRrule("FREQ=WEEKLY", MONDAY);
    expect(parsed?.weekdays).toEqual([0]); // Monday
  });

  it("plain FREQ=MONTHLY defaults to dtstart's day-of-month", () => {
    const parsed = parseRrule("FREQ=MONTHLY", "2026-01-15T09:00");
    expect(parsed).toMatchObject({ monthlyMode: "monthday", monthday: 15 });
  });
});

describe("parseRrule — unsupported constructs fall back to raw mode (null)", () => {
  const unsupported = [
    // sub-daily cadence stays raw-only (sharp edge for an agent-run scheduler)
    "FREQ=HOURLY",
    "FREQ=MINUTELY",
    "FREQ=SECONDLY",
    // explicit clock parts would silently disagree with dtstart
    "FREQ=DAILY;BYHOUR=9",
    "FREQ=WEEKLY;BYDAY=MO;BYMINUTE=30",
    // constructs the widget has no controls for
    "FREQ=MONTHLY;BYDAY=MO;BYSETPOS=1",
    "FREQ=YEARLY;BYWEEKNO=20",
    "FREQ=YEARLY;BYYEARDAY=100",
    "FREQ=YEARLY;BYMONTH=1,2",
    "FREQ=WEEKLY;WKST=MO",
    // shapes outside the widget's monthly/weekly model
    "FREQ=MONTHLY;BYMONTHDAY=1,15",
    "FREQ=MONTHLY;BYMONTHDAY=-1",
    "FREQ=MONTHLY;BYDAY=MO,TU",
    "FREQ=WEEKLY;BYDAY=2MO",
    "FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR",
    // invalid per the API validator
    "FREQ=DAILY;INTERVAL=0",
    "COUNT=5",
    "garbage",
    // UNTIL with a time-of-day the widget would rewrite to end-of-day
    "FREQ=DAILY;UNTIL=20261226T090000Z",
    "FREQ=DAILY;UNTIL=20261226",
    // RFC 5545 forbids UNTIL and COUNT together
    "FREQ=DAILY;COUNT=5;UNTIL=20261226T000000Z",
    // embedded DTSTART lines are out of widget scope
    "DTSTART:20260105T090000Z\nRRULE:FREQ=DAILY",
  ];

  it.each(unsupported)("returns null for %j", (rule) => {
    expect(parseRrule(rule, MONDAY)).toBeNull();
  });
});
