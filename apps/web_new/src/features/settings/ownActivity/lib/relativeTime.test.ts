// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Own-history timestamps (parity/specs/profile.md §7, PROF-019): recent
// rows read as a relative age, older rows as a fixed date in the
// member's zone.
import { describe, expect, it } from "vitest";

import { activityAge, activityDate } from "./relativeTime.js";

const NOW = Date.parse("2026-10-19T12:00:00Z");

function ago(ms: number): string {
  return new Date(NOW - ms).toISOString();
}

describe("activityDate", () => {
  it("formats an absolute date in the given zone", () => {
    expect(activityDate("2026-10-14T23:30:00Z", "UTC")).toBe("Oct 14, 2026");
    expect(activityDate("2026-10-14T23:30:00Z", "America/Los_Angeles")).toBe("Oct 14, 2026");
    expect(activityDate("2026-10-14T01:30:00Z", "America/Los_Angeles")).toBe("Oct 13, 2026");
  });

  it("uses the runtime zone when the member has none", () => {
    expect(activityDate("2026-10-14T12:00:00Z", undefined)).toMatch(/Oct 1[34], 2026/);
    expect(activityDate("2026-10-14T12:00:00Z", "")).toMatch(/Oct 1[34], 2026/);
  });

  it("is empty for an unreadable timestamp", () => {
    expect(activityDate("not a date", "UTC")).toBe("");
  });
});

describe("activityAge", () => {
  it("reads the last moments as just now", () => {
    expect(activityAge(ago(0), NOW, "UTC")).toBe("just now");
    expect(activityAge(ago(59_000), NOW, "UTC")).toBe("just now");
    expect(activityAge(ago(-60_000), NOW, "UTC")).toBe("just now");
  });

  it("counts minutes and hours with singular forms", () => {
    expect(activityAge(ago(60_000), NOW, "UTC")).toBe("1 minute ago");
    expect(activityAge(ago(5 * 60_000), NOW, "UTC")).toBe("5 minutes ago");
    expect(activityAge(ago(60 * 60_000), NOW, "UTC")).toBe("1 hour ago");
    expect(activityAge(ago(5 * 60 * 60_000), NOW, "UTC")).toBe("5 hours ago");
  });

  it("counts days inside the week", () => {
    expect(activityAge(ago(25 * 60 * 60_000), NOW, "UTC")).toBe("yesterday");
    expect(activityAge(ago(2 * 24 * 60 * 60_000), NOW, "UTC")).toBe("2 days ago");
    expect(activityAge(ago(3 * 24 * 60 * 60_000), NOW, "UTC")).toBe("3 days ago");
  });

  it("uses yesterday for the single past day", () => {
    expect(activityAge(ago(24 * 60 * 60_000), NOW, "UTC")).toBe("yesterday");
  });

  it("falls back to a fixed date beyond a week", () => {
    expect(activityAge(ago(7 * 24 * 60 * 60_000), NOW, "UTC")).toBe("Oct 12, 2026");
    expect(activityAge(ago(30 * 24 * 60 * 60_000), NOW, "UTC")).toBe("Sep 19, 2026");
  });

  it("is empty for an unreadable timestamp", () => {
    expect(activityAge("nope", NOW, "UTC")).toBe("");
  });
});
