// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Panel text (parity/specs/agents.md §5, AGT-017): an instant is shown in
// the install's zone, a future firing reads Scheduled, a past one reports
// its run status, and a past run gets a detail link.
import { describe, expect, it } from "vitest";

import {
  formatOccurrenceInstant,
  occurrenceContext,
  occurrenceRecurrence,
  occurrenceRunHref,
  occurrenceStatus,
} from "./calendarText.js";

describe("formatOccurrenceInstant", () => {
  it("reads the instant in the install's zone", () => {
    // 2026-10-26T20:00Z is 27 October 09:00 in Auckland (UTC+13).
    const text = formatOccurrenceInstant("2026-10-26T20:00:00.000Z", "Pacific/Auckland");
    expect(text).toContain("Oct 27");
    expect(text).toContain("9:00");
    expect(text).toContain("GMT+13:00");
  });

  it("names UTC instead of a zero offset", () => {
    expect(formatOccurrenceInstant("2026-10-26T09:00:00.000Z", "UTC")).toContain("UTC");
  });

  it("falls back to UTC for an unknown zone and blanks a bad instant", () => {
    expect(formatOccurrenceInstant("2026-10-26T09:00:00.000Z", "Not/AZone")).toBe("2026-10-26T09:00:00.000Z");
    expect(formatOccurrenceInstant("not-a-date", "UTC")).toBe("");
  });
});

describe("occurrenceStatus", () => {
  it("names a future firing Scheduled", () => {
    expect(occurrenceStatus({ kind: "scheduled", status: null })).toBe("Scheduled");
  });

  it("reports a past run's status, or Past when it has none", () => {
    expect(occurrenceStatus({ kind: "past", status: "completed" })).toBe("completed");
    expect(occurrenceStatus({ kind: "past", status: "  " })).toBe("Past");
    expect(occurrenceStatus({ kind: "past", status: null })).toBe("Past");
  });
});

describe("install context", () => {
  it("renders the rule in plain language", () => {
    expect(occurrenceRecurrence({ rrule: "FREQ=DAILY" })).toBe("every day");
    expect(occurrenceRecurrence({ rrule: "" })).toBe("Once at dtstart");
    expect(occurrenceRecurrence(null)).toBe("");
  });

  it("falls back to a quiet placeholder for empty context", () => {
    expect(occurrenceContext({ extra_context: "  Frame it.  " })).toBe("Frame it.");
    expect(occurrenceContext({ extra_context: "   " })).toBe("No extra context");
    expect(occurrenceContext(null)).toBe("No extra context");
  });
});

describe("occurrenceRunHref", () => {
  it("links a past firing to its run detail", () => {
    expect(occurrenceRunHref("acme", "p1", { agent_run_id: "r1" })).toBe("/acme/projects/p1/runners/detail/r1");
  });

  it("has no link without a run", () => {
    expect(occurrenceRunHref("acme", "p1", { agent_run_id: null })).toBeNull();
  });
});
