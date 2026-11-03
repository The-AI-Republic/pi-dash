// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Recurrence builder rules (parity/specs/agents.md §14, AGT-065): the
// stored rule is read into widget state, an unreadable rule stays raw and
// byte-identical, and widget state serializes back to the bare API form.
import { describe, expect, it } from "vitest";

import {
  DEFAULT_AFTER_COUNT,
  WEEKDAY_CODES,
  WEEKDAY_LABELS,
  UNIT_LABELS,
  REPEAT_UNITS,
  defaultEndDate,
  defaultRecurrence,
  monthlyForStart,
  ordinalForDay,
  parseRecurrence,
  serializeRecurrence,
  withMonthlyMode,
  withUnit,
  withWeekdayToggled,
  type RecurrenceState,
} from "./recurrence.js";

const START = "2026-10-19T09:30";

function state(overrides: Partial<RecurrenceState> = {}): RecurrenceState {
  return { ...defaultRecurrence(START), ...overrides };
}

describe("parseRecurrence", () => {
  it("reads an empty rule as the single-shot form", () => {
    expect(parseRecurrence("", START)).toEqual({ state: state({ unit: "none" }), expressible: true });
    expect(parseRecurrence("   ", START)).toEqual({ state: state({ unit: "none" }), expressible: true });
  });

  it("reads the bare frequency form with its interval", () => {
    expect(parseRecurrence("FREQ=DAILY", START).state).toMatchObject({ unit: "day", interval: 1 });
    expect(parseRecurrence("freq=weekly;interval=2", START).state).toMatchObject({ unit: "week", interval: 2 });
  });

  it("reads weekly days Sunday-first", () => {
    const parsed = parseRecurrence("FREQ=WEEKLY;BYDAY=FR,MO", START);
    expect(parsed.expressible).toBe(true);
    expect(parsed.state.weekdays).toEqual(["MO", "FR"]);
    expect(WEEKDAY_CODES).toEqual(["SU", "MO", "TU", "WE", "TH", "FR", "SA"]);
  });

  it("reads a monthly day and defaults to the start's day", () => {
    expect(parseRecurrence("FREQ=MONTHLY", START).state.monthly).toEqual({ on: "day", day: 19 });
    expect(parseRecurrence("FREQ=MONTHLY;BYMONTHDAY=15", START).state.monthly).toEqual({ on: "day", day: 15 });
  });

  it("reads a monthly nth-weekday as its ordinal form", () => {
    expect(parseRecurrence("FREQ=MONTHLY;BYDAY=2MO", START).state.monthly).toEqual({
      on: "weekday",
      ordinal: 2,
      weekday: "MO",
    });
    expect(parseRecurrence("FREQ=MONTHLY;BYDAY=-1FR", START).state.monthly).toEqual({
      on: "weekday",
      ordinal: -1,
      weekday: "FR",
    });
  });

  it("reads both end forms, including the end-of-day stamp", () => {
    expect(parseRecurrence("FREQ=DAILY;COUNT=12", START).state.ends).toEqual({ mode: "after", count: 12 });
    expect(parseRecurrence("FREQ=DAILY;UNTIL=20271231T235959Z", START).state.ends).toEqual({
      mode: "on",
      date: "2027-12-31",
    });
    expect(parseRecurrence("FREQ=DAILY;UNTIL=20271231", START).state.ends).toEqual({
      mode: "on",
      date: "2027-12-31",
    });
  });

  it("marks every rule it cannot express as raw", () => {
    const inexpressible = [
      "RRULE:FREQ=DAILY",
      "FREQ=DAILY;",
      "=DAILY",
      "FREQ=",
      "FREQ=DAILY;FREQ=WEEKLY",
      "FREQ=HOURLY",
      "FREQ=NOPE",
      "FREQ=DAILY;INTERVAL=x",
      "FREQ=DAILY;INTERVAL=0",
      "FREQ=DAILY;BYHOUR=9",
      "FREQ=DAILY;BYDAY=MO",
      "FREQ=DAILY;BYMONTHDAY=1",
      "FREQ=WEEKLY;BYMONTHDAY=1",
      "FREQ=WEEKLY;BYDAY=XX",
      "FREQ=WEEKLY;BYDAY=MO,MO",
      "FREQ=WEEKLY;BYDAY=",
      "FREQ=MONTHLY;BYDAY=2XX",
      "FREQ=MONTHLY;BYDAY=9MO",
      "FREQ=MONTHLY;BYDAY=MO",
      "FREQ=MONTHLY;BYDAY=2MO;BYMONTHDAY=1",
      "FREQ=MONTHLY;BYMONTHDAY=32",
      "FREQ=MONTHLY;BYMONTHDAY=-1",
      "FREQ=DAILY;COUNT=0",
      "FREQ=DAILY;COUNT=x",
      "FREQ=DAILY;COUNT=2;UNTIL=20271231T235959Z",
      "FREQ=DAILY;UNTIL=20271301T235959Z",
      "FREQ=DAILY;UNTIL=NOPE",
    ];
    for (const rule of inexpressible) {
      expect(parseRecurrence(rule, START).expressible, rule).toBe(false);
    }
  });

  it("still yields a usable default state for a raw rule", () => {
    const parsed = parseRecurrence("FREQ=HOURLY", START);
    expect(parsed.state.unit).toBe("day");
    expect(parsed.state.interval).toBe(1);
    expect(parsed.state.weekdays).toEqual(["MO"]);
  });

  it("falls back to today for an unreadable start", () => {
    const parsed = parseRecurrence("FREQ=WEEKLY", "not-a-date");
    expect(WEEKDAY_CODES).toContain(parsed.state.weekdays[0]);
    expect(parsed.state.monthly.on).toBe("day");
  });
});

describe("serializeRecurrence", () => {
  it("writes the bare frequency form for every unit", () => {
    expect(serializeRecurrence(state({ unit: "day" }))).toBe("FREQ=DAILY");
    expect(serializeRecurrence(state({ unit: "week", weekdays: ["MO", "FR"] }))).toBe("FREQ=WEEKLY;BYDAY=MO,FR");
    expect(serializeRecurrence(state({ unit: "month", monthly: { on: "day", day: 15 } }))).toBe(
      "FREQ=MONTHLY;BYMONTHDAY=15"
    );
    expect(serializeRecurrence(state({ unit: "month", monthly: { on: "weekday", ordinal: 3, weekday: "WE" } }))).toBe(
      "FREQ=MONTHLY;BYDAY=3WE"
    );
    expect(serializeRecurrence(state({ unit: "year" }))).toBe("FREQ=YEARLY");
    expect(serializeRecurrence(state({ unit: "none" }))).toBe("");
  });

  it("omits an interval of one and clamps smaller values up", () => {
    expect(serializeRecurrence(state({ interval: 1 }))).toBe("FREQ=DAILY");
    expect(serializeRecurrence(state({ interval: 3 }))).toBe("FREQ=DAILY;INTERVAL=3");
    expect(serializeRecurrence(state({ interval: 0 }))).toBe("FREQ=DAILY");
    expect(serializeRecurrence(state({ interval: -2 }))).toBe("FREQ=DAILY");
    expect(serializeRecurrence(state({ interval: 2.7 }))).toBe("FREQ=DAILY;INTERVAL=2");
    expect(serializeRecurrence(state({ interval: Number.NaN }))).toBe("FREQ=DAILY");
  });

  it("writes both end forms and leaves never empty", () => {
    expect(serializeRecurrence(state({ ends: { mode: "never" } }))).toBe("FREQ=DAILY");
    expect(serializeRecurrence(state({ ends: { mode: "on", date: "2027-12-31" } }))).toBe(
      "FREQ=DAILY;UNTIL=20271231T235959Z"
    );
    expect(serializeRecurrence(state({ ends: { mode: "after", count: 4 } }))).toBe("FREQ=DAILY;COUNT=4");
    expect(serializeRecurrence(state({ ends: { mode: "after", count: 0 } }))).toBe("FREQ=DAILY;COUNT=1");
  });

  it("ignores an unparseable end date rather than writing a broken rule", () => {
    expect(serializeRecurrence(state({ ends: { mode: "on", date: "31/12/2027" } }))).toBe("FREQ=DAILY");
  });

  it("keeps weekly days in Sunday-first order and never writes an empty set", () => {
    expect(serializeRecurrence(state({ unit: "week", weekdays: ["SA", "SU"] }))).toBe("FREQ=WEEKLY;BYDAY=SU,SA");
    expect(serializeRecurrence(state({ unit: "week", weekdays: [] }))).toBe("FREQ=WEEKLY;BYDAY=MO");
  });

  it("clamps a monthly day into the calendar range", () => {
    expect(serializeRecurrence(state({ unit: "month", monthly: { on: "day", day: 0 } }))).toBe(
      "FREQ=MONTHLY;BYMONTHDAY=1"
    );
    expect(serializeRecurrence(state({ unit: "month", monthly: { on: "day", day: 40 } }))).toBe(
      "FREQ=MONTHLY;BYMONTHDAY=31"
    );
  });
});

describe("round trip", () => {
  it("re-serializes an expressible rule to the same meaning", () => {
    const rules = [
      "FREQ=DAILY",
      "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR",
      "FREQ=MONTHLY;BYMONTHDAY=15",
      "FREQ=MONTHLY;BYDAY=2MO",
      "FREQ=YEARLY;COUNT=10",
      "FREQ=DAILY;UNTIL=20271231T235959Z",
    ];
    for (const rule of rules) {
      const parsed = parseRecurrence(rule, START);
      expect(parsed.expressible, rule).toBe(true);
      expect(serializeRecurrence(parsed.state)).toBe(rule);
    }
  });
});

describe("helper state transitions", () => {
  it("switches the unit while keeping the rest of the state", () => {
    const before = state({ interval: 2, ends: { mode: "after", count: 5 } });
    expect(withUnit(before, "week")).toMatchObject({ unit: "week", interval: 2, ends: { mode: "after", count: 5 } });
  });

  it("toggles weekday chips and refuses to empty them", () => {
    const weekly = state({ unit: "week", weekdays: ["MO"] });
    expect(withWeekdayToggled(weekly, "WE").weekdays).toEqual(["MO", "WE"]);
    expect(withWeekdayToggled(weekly, "SU").weekdays).toEqual(["SU", "MO"]);
    expect(withWeekdayToggled(weekly, "MO")).toBe(weekly);
    expect(withWeekdayToggled(state({ unit: "week", weekdays: ["MO", "FR"] }), "FR").weekdays).toEqual(["MO"]);
  });

  it("derives the monthly default from the start and the ordinal from its day", () => {
    expect(ordinalForDay(1)).toBe(1);
    expect(ordinalForDay(17)).toBe(3);
    expect(ordinalForDay(29)).toBe(5);
    expect(ordinalForDay(31)).toBe(5);
    expect(ordinalForDay(0)).toBe(1);
    expect(monthlyForStart("2026-10-21T09:00", { on: "day", day: 1 })).toEqual({ on: "day", day: 21 });
    expect(monthlyForStart("2026-10-21T09:00", { on: "weekday", ordinal: 1, weekday: "MO" })).toEqual({
      on: "weekday",
      ordinal: 3,
      weekday: "WE",
    });
  });

  it("keeps the monthly form when the requested form is already active", () => {
    const weekday = state({ unit: "month", monthly: { on: "weekday", ordinal: 2, weekday: "FR" } });
    expect(withMonthlyMode(weekday, "weekday")).toBe(weekday);
    const day = state({ unit: "month", monthly: { on: "day", day: 12 } });
    expect(withMonthlyMode(day, "day")).toBe(day);
  });

  it("seeds the nth-weekday form from the day and the first weekly chip", () => {
    const weekly = state({ unit: "week", weekdays: ["WE"], monthly: { on: "day", day: 17 } });
    expect(withMonthlyMode(weekly, "weekday").monthly).toEqual({ on: "weekday", ordinal: 3, weekday: "WE" });
    expect(withMonthlyMode(state({ unit: "month", monthly: { on: "day", day: 4 } }), "weekday").monthly).toEqual({
      on: "weekday",
      ordinal: 1,
      weekday: "MO",
    });
    const bare = state({ unit: "week", weekdays: [], monthly: { on: "day", day: 4 } });
    expect(withMonthlyMode(bare, "weekday").monthly).toEqual({ on: "weekday", ordinal: 1, weekday: "MO" });
  });

  it("seeds the day form from the nth-weekday ordinal", () => {
    expect(
      withMonthlyMode(state({ unit: "month", monthly: { on: "weekday", ordinal: -1, weekday: "FR" } }), "day").monthly
    ).toEqual({ on: "day", day: 28 });
    expect(
      withMonthlyMode(state({ unit: "month", monthly: { on: "weekday", ordinal: 5, weekday: "MO" } }), "day").monthly
    ).toEqual({ on: "day", day: 29 });
  });

  it("defaults the end date to one year after the start", () => {
    expect(defaultEndDate("2026-10-19T09:30")).toBe("2027-10-19");
    expect(defaultEndDate("bad")).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("offers a near-term prefill for an after-N series", () => {
    expect(DEFAULT_AFTER_COUNT).toBe(10);
  });

  it("names every unit and weekday for the controls", () => {
    expect(REPEAT_UNITS).toEqual(["none", "day", "week", "month", "year"]);
    expect(Object.values(UNIT_LABELS)).toEqual(["Does not repeat", "Day", "Week", "Month", "Year"]);
    expect(Object.values(WEEKDAY_LABELS)).toEqual([
      "Sunday",
      "Monday",
      "Tuesday",
      "Wednesday",
      "Thursday",
      "Friday",
      "Saturday",
    ]);
  });
});
