// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_TZID,
  browserTimeZone,
  defaultStartLocal,
  describeRrule,
  describeStoredRrule,
  emptySchedule,
  hasScheduleErrors,
  isoToLocalStart,
  localStartToIso,
  schedulePayload,
  timeZoneOptions,
  validateSchedule,
} from "./schedule.js";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("start time round trip", () => {
  it("defaults to tomorrow at 09:00 local", () => {
    const value = defaultStartLocal(new Date(2026, 9, 18, 15, 42));
    expect(value).toBe("2026-10-19T09:00");
  });

  it("converts a local wall time to the UTC instant and back", () => {
    const iso = localStartToIso("2026-10-19T09:30");
    expect(iso).toBe(new Date(2026, 9, 19, 9, 30).toISOString());
    expect(isoToLocalStart(iso)).toBe("2026-10-19T09:30");
  });

  it("treats blank and invalid values as empty", () => {
    expect(localStartToIso("  ")).toBe("");
    expect(localStartToIso("not-a-date")).toBe("");
    expect(isoToLocalStart("nope")).toBe("");
  });
});

describe("schedule validation", () => {
  it("requires a start and accepts a valid one", () => {
    expect(validateSchedule({ ...emptySchedule(new Date(2026, 9, 18)), startsAt: "" })).toEqual({
      startsAt: "Start time is required.",
    });
    expect(validateSchedule({ ...emptySchedule(new Date(2026, 9, 18)), startsAt: "garbage" })).toEqual({
      startsAt: "Enter a valid start time.",
    });
    expect(validateSchedule(emptySchedule(new Date(2026, 9, 18)))).toEqual({});
  });

  it("reports whether any error remains", () => {
    expect(hasScheduleErrors({})).toBe(false);
    expect(hasScheduleErrors({ startsAt: "x" })).toBe(true);
  });
});

describe("schedule payload", () => {
  it("trims the rule and context and keeps the zone", () => {
    expect(
      schedulePayload({
        startsAt: "2026-10-19T09:30",
        timeZone: "",
        rrule: "  FREQ=DAILY  ",
        projectContext: "  Ship on Monday  ",
        enabled: false,
      })
    ).toEqual({
      dtstart: new Date(2026, 9, 19, 9, 30).toISOString(),
      tzid: DEFAULT_TZID,
      rrule: "FREQ=DAILY",
      extra_context: "Ship on Monday",
      enabled: false,
    });
  });

  it("builds an enabled daily schedule with the browser zone", () => {
    const value = emptySchedule(new Date(2026, 9, 18, 10, 0));
    expect(value).toMatchObject({ startsAt: "2026-10-19T09:00", rrule: "FREQ=DAILY", enabled: true });
    expect(value.timeZone).toBe(browserTimeZone());
  });
});

describe("time zones", () => {
  it("falls back to UTC when the runtime cannot report a zone", () => {
    const formatter = vi.spyOn(Intl, "DateTimeFormat").mockImplementation((() => {
      throw new Error("no intl");
    }) as never);
    expect(browserTimeZone()).toBe(DEFAULT_TZID);
    formatter.mockRestore();
    expect(browserTimeZone()).not.toBe("");
  });

  it("lists UTC once, sorted", () => {
    const zones = timeZoneOptions();
    expect(zones[0]).toBe(DEFAULT_TZID);
    expect(new Set(zones).size).toBe(zones.length);
    expect([...zones.slice(1)].sort((a, b) => a.localeCompare(b))).toEqual(zones.slice(1));
  });

  it("uses the curated fallback when supportedValuesOf is missing or throws", () => {
    const original = Intl.supportedValuesOf;
    const intl = Intl as unknown as { supportedValuesOf: ((key: "timeZone") => string[]) | undefined };
    intl.supportedValuesOf = undefined;
    expect(timeZoneOptions()).toEqual([DEFAULT_TZID]);
    intl.supportedValuesOf = () => {
      throw new Error("unsupported");
    };
    expect(timeZoneOptions()).toEqual([DEFAULT_TZID]);
    intl.supportedValuesOf = original;
  });
});

describe("browserTimeZone", () => {
  it("falls back to UTC when the browser cannot report a zone", () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, "resolvedOptions").mockImplementation(() => {
      throw new Error("no zone");
    });
    expect(browserTimeZone()).toBe(DEFAULT_TZID);
  });

  it("falls back to UTC on an empty zone name", () => {
    vi.spyOn(Intl.DateTimeFormat.prototype, "resolvedOptions").mockImplementation(
      () => ({ timeZone: "" }) as Intl.ResolvedDateTimeFormatOptions
    );
    expect(browserTimeZone()).toBe(DEFAULT_TZID);
  });
});

describe("describeRrule", () => {
  it("describes single and recurring rules", () => {
    expect(describeRrule("")).toBe("Fires once at the start time");
    expect(describeRrule("FREQ=DAILY")).toBe("every day");
    expect(describeRrule("RRULE:FREQ=WEEKLY;BYDAY=MO")).toBe("every week on Monday");
    expect(describeRrule("FREQ=WEEKLY;BYDAY=MO,WE")).toBe("every week on Monday, Wednesday");
    expect(describeRrule("FREQ=WEEKLY;BYDAY=MO,TU,WE")).toBe("every week on Monday, Tuesday, Wednesday");
    expect(describeRrule("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR")).toBe("every weekday");
    expect(describeRrule("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR")).toBe("every 2 weeks on Monday, Friday");
    expect(describeRrule("FREQ=DAILY;INTERVAL=2")).toBe("every 2 days");
    expect(describeRrule("freq=daily;byhour=9;byminute=5")).toBe("every day at 09:05");
    expect(describeRrule("FREQ=MONTHLY;BYMONTHDAY=1,15")).toBe("every month on day 1, 15");
    expect(describeRrule("FREQ=HOURLY;COUNT=3")).toBe("every hour for 3 times");
    expect(describeRrule("FREQ=YEARLY;COUNT=1")).toBe("every year for 1 time");
    expect(describeRrule("FREQ=DAILY;UNTIL=20270101T090000Z")).toBe("every day until 2027-01-01");
    expect(describeRrule("FREQ=MINUTELY")).toBe("every minute");
  });

  it("reads a negative and a zero interval as unreadable", () => {
    expect(describeRrule("FREQ=DAILY;INTERVAL=-1")).toBe("FREQ=DAILY;INTERVAL=-1");
    expect(describeRrule("FREQ=DAILY;INTERVAL=0")).toBe("FREQ=DAILY;INTERVAL=0");
  });

  it("returns unreadable rules verbatim", () => {
    expect(describeRrule("FREQ=NOPE")).toBe("FREQ=NOPE");
    expect(describeRrule("FREQ=SECONDLY")).toBe("FREQ=SECONDLY");
    expect(describeRrule("FREQ=DAILY;NOVALUE")).toBe("FREQ=DAILY;NOVALUE");
    expect(describeRrule("BYDAY=MO")).toBe("BYDAY=MO");
    expect(describeRrule("FREQ=DAILY;INTERVAL=0")).toBe("FREQ=DAILY;INTERVAL=0");
    expect(describeRrule("FREQ=DAILY;INTERVAL=x")).toBe("FREQ=DAILY;INTERVAL=x");
    expect(describeRrule("FREQ=WEEKLY;BYDAY=XX")).toBe("FREQ=WEEKLY;BYDAY=XX");
    expect(describeRrule("FREQ=MONTHLY;BYMONTHDAY=first")).toBe("FREQ=MONTHLY;BYMONTHDAY=first");
    expect(describeRrule("FREQ=DAILY;;BYHOUR=9")).toBe("every day at 09:00");
    expect(describeRrule("FREQ=DAILY;UNTIL=garbage")).toBe("every day");
    expect(describeRrule("FREQ=DAILY;COUNT=0")).toBe("every day");
  });
});

describe("describeStoredRrule", () => {
  it("names a blank stored rule the same way the dialog does", () => {
    expect(describeStoredRrule("")).toBe("Fires once at the start time");
    expect(describeStoredRrule("   ")).toBe("Fires once at the start time");
  });

  it("delegates every non-blank rule to the plain-language reading", () => {
    expect(describeStoredRrule("FREQ=DAILY")).toBe("every day");
    expect(describeStoredRrule("FREQ=NOPE")).toBe("FREQ=NOPE");
  });
});
