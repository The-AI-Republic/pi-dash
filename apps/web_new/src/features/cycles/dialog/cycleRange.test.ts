// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  CYCLE_OVERLAP_TITLE,
  cycleDaysComplete,
  cycleDaysUnchanged,
  cycleFailureMessage,
  cycleFormDays,
  cycleOverlapMessage,
  needsCycleDateCheck,
  validateCycleRange,
} from "./cycleRange.js";

describe("cycleFormDays", () => {
  it("passes picked days through and nulls the unset ends", () => {
    expect(cycleFormDays({ start: "2026-11-01", end: "2026-11-30" })).toEqual({
      start_date: "2026-11-01",
      end_date: "2026-11-30",
    });
    expect(cycleFormDays({ start: "2026-11-01", end: null })).toEqual({
      start_date: "2026-11-01",
      end_date: null,
    });
    expect(cycleFormDays({ start: null, end: null })).toEqual({ start_date: null, end_date: null });
  });

  it("reads blank ends as unset", () => {
    expect(cycleFormDays({ start: "", end: "" })).toEqual({ start_date: null, end_date: null });
  });
});

describe("cycleDaysComplete", () => {
  it("needs both days", () => {
    expect(cycleDaysComplete({ start_date: "2026-11-01", end_date: "2026-11-30" })).toBe(true);
    expect(cycleDaysComplete({ start_date: "2026-11-01", end_date: null })).toBe(false);
    expect(cycleDaysComplete({ start_date: null, end_date: null })).toBe(false);
  });
});

describe("cycleDaysUnchanged", () => {
  it("matches equal days through the project timezone", () => {
    expect(
      cycleDaysUnchanged(
        { start_date: "2026-11-01T12:00:00Z", end_date: "2026-11-30T12:00:00Z" },
        { start_date: "2026-11-01", end_date: "2026-11-30" },
        "UTC"
      )
    ).toBe(true);
  });

  it("reports a moved or cleared side as changed", () => {
    expect(
      cycleDaysUnchanged(
        { start_date: "2026-11-01T12:00:00Z", end_date: "2026-11-30T12:00:00Z" },
        { start_date: "2026-11-02", end_date: "2026-11-30" },
        "UTC"
      )
    ).toBe(false);
    expect(
      cycleDaysUnchanged(
        { start_date: "2026-11-01T12:00:00Z", end_date: "2026-11-30T12:00:00Z" },
        { start_date: null, end_date: null },
        "UTC"
      )
    ).toBe(false);
  });

  it("matches stored nulls only against unset picks", () => {
    expect(cycleDaysUnchanged({ start_date: null, end_date: null }, { start_date: null, end_date: null }, "UTC")).toBe(
      true
    );
    expect(
      cycleDaysUnchanged({ start_date: null, end_date: null }, { start_date: "2026-11-01", end_date: null }, "UTC")
    ).toBe(false);
  });

  it("treats a broken stored instant as unset", () => {
    expect(
      cycleDaysUnchanged({ start_date: "not-a-date", end_date: null }, { start_date: null, end_date: null }, "UTC")
    ).toBe(true);
  });
});

describe("needsCycleDateCheck", () => {
  it("checks a dated create and skips undated or one-sided creates", () => {
    expect(needsCycleDateCheck(null, { start: "2026-11-01", end: "2026-11-30" }, "UTC")).toBe(true);
    expect(needsCycleDateCheck(null, { start: "2026-11-01", end: null }, "UTC")).toBe(false);
    expect(needsCycleDateCheck(null, { start: null, end: null }, "UTC")).toBe(false);
  });

  it("skips untouched or cleared edit ranges and checks changed ones", () => {
    const original = { start_date: "2026-11-01T12:00:00Z", end_date: "2026-11-30T12:00:00Z" };
    expect(needsCycleDateCheck(original, { start: "2026-11-01", end: "2026-11-30" }, "UTC")).toBe(false);
    expect(needsCycleDateCheck(original, { start: null, end: null }, "UTC")).toBe(false);
    expect(needsCycleDateCheck(original, { start: "2026-11-01", end: null }, "UTC")).toBe(false);
    expect(needsCycleDateCheck(original, { start: "2026-11-01", end: "2026-12-01" }, "UTC")).toBe(true);
  });
});

describe("cycleOverlapMessage", () => {
  it("pins the refusal title", () => {
    expect(CYCLE_OVERLAP_TITLE).toBe("Error!");
  });

  it("names the pinned phrase and the draft workaround in both modes", () => {
    for (const mode of ["create", "update"] as const) {
      const message = cycleOverlapMessage(mode);
      expect(message).toContain("you already have a cycle");
      expect(message).toMatch(/draft/i);
    }
    expect(cycleOverlapMessage("create")).toContain("Leave the new cycle undated");
    expect(cycleOverlapMessage("update")).toContain("Clear this cycle's dates");
  });
});

describe("validateCycleRange", () => {
  it("refuses a one-sided create range naming both ends", () => {
    expect(validateCycleRange({ start: "2026-11-01", end: null }, "create")).toContain("both");
    expect(validateCycleRange({ start: null, end: "2026-11-30" }, "create")).toContain("both");
    expect(validateCycleRange({ start: "2026-11-01", end: "" }, "create")).toContain("both");
  });

  it("accepts undated and two-sided creates and every edit range", () => {
    expect(validateCycleRange({ start: null, end: null }, "create")).toBe(null);
    expect(validateCycleRange({ start: "2026-11-01", end: "2026-11-30" }, "create")).toBe(null);
    expect(validateCycleRange({ start: "2026-11-01", end: null }, "update")).toBe(null);
    expect(validateCycleRange({ start: null, end: null }, "update")).toBe(null);
  });
});

describe("cycleFailureMessage", () => {
  it("prefers the server detail", () => {
    expect(cycleFailureMessage({ body: { detail: "Name taken." } }, "create")).toBe("Name taken.");
  });

  it("reads the server error key when no detail is present", () => {
    expect(cycleFailureMessage({ body: { error: "Both start date and end date are required." } }, "create")).toBe(
      "Both start date and end date are required."
    );
    expect(cycleFailureMessage({ body: { detail: "Name taken.", error: "Other." } }, "create")).toBe("Name taken.");
    expect(cycleFailureMessage({ body: { error: "" } }, "update")).toBe(
      "Nothing was changed — check the details and try again."
    );
  });

  it("falls back to the fixed copy per mode", () => {
    expect(cycleFailureMessage(new Error("boom"), "create")).toBe(
      "Nothing was created — check the details and try again."
    );
    expect(cycleFailureMessage(new Error("boom"), "update")).toBe(
      "Nothing was changed — check the details and try again."
    );
    expect(cycleFailureMessage({ body: { detail: 42 } }, "create")).toBe(
      "Nothing was created — check the details and try again."
    );
    expect(cycleFailureMessage({ body: "oops" }, "update")).toBe(
      "Nothing was changed — check the details and try again."
    );
    expect(cycleFailureMessage({ body: ["oops"] }, "create")).toBe(
      "Nothing was created — check the details and try again."
    );
    expect(cycleFailureMessage({ body: { detail: "" } }, "create")).toBe(
      "Nothing was created — check the details and try again."
    );
  });
});
