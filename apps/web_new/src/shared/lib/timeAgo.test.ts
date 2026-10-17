// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { timeAgo } from "./timeAgo.js";

const NOW = Date.parse("2026-10-17T12:00:00Z");

function ago(ms: number): string {
  return new Date(NOW - ms).toISOString();
}

describe("timeAgo", () => {
  it("reads recent, future, and broken input as now", () => {
    expect(timeAgo(ago(0), NOW)).toBe("just now");
    expect(timeAgo(ago(30_000), NOW)).toBe("just now");
    expect(timeAgo(new Date(NOW + 60_000).toISOString(), NOW)).toBe("just now");
    expect(timeAgo("not a timestamp", NOW)).toBe("just now");
  });

  it("counts minutes and hours with singular forms", () => {
    expect(timeAgo(ago(60_000), NOW)).toBe("1 minute ago");
    expect(timeAgo(ago(4 * 60_000 + 30_000), NOW)).toBe("4 minutes ago");
    expect(timeAgo(ago(59 * 60_000), NOW)).toBe("59 minutes ago");
    expect(timeAgo(ago(60 * 60_000), NOW)).toBe("1 hour ago");
    expect(timeAgo(ago(5 * 3_600_000), NOW)).toBe("5 hours ago");
    expect(timeAgo(ago(23 * 3_600_000), NOW)).toBe("23 hours ago");
  });

  it("names yesterday, then counts days", () => {
    expect(timeAgo(ago(24 * 3_600_000), NOW)).toBe("yesterday");
    expect(timeAgo(ago(36 * 3_600_000), NOW)).toBe("yesterday");
    expect(timeAgo(ago(2 * 86_400_000), NOW)).toBe("2 days ago");
    expect(timeAgo(ago(6 * 86_400_000), NOW)).toBe("6 days ago");
  });

  it("counts weeks, months, and years with singular forms", () => {
    expect(timeAgo(ago(7 * 86_400_000), NOW)).toBe("1 week ago");
    expect(timeAgo(ago(21 * 86_400_000), NOW)).toBe("3 weeks ago");
    expect(timeAgo(ago(30 * 86_400_000), NOW)).toBe("1 month ago");
    expect(timeAgo(ago(90 * 86_400_000), NOW)).toBe("3 months ago");
    expect(timeAgo(ago(365 * 86_400_000), NOW)).toBe("1 year ago");
    expect(timeAgo(ago(800 * 86_400_000), NOW)).toBe("2 years ago");
  });
});
