// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { formatRelativeVisitTime } from "./relativeTime.js";

const NOW = Date.parse("2026-10-14T12:00:00Z");

function stamp(offsetMs: number): string {
  return new Date(NOW - offsetMs).toISOString();
}

describe("formatRelativeVisitTime", () => {
  it("reads sub-minute visits as just now", () => {
    expect(formatRelativeVisitTime(stamp(0), NOW)).toBe("just now");
    expect(formatRelativeVisitTime(stamp(44_000), NOW)).toBe("just now");
  });

  it("tolerates future stamps and unparseable input", () => {
    expect(formatRelativeVisitTime(new Date(NOW + 60_000).toISOString(), NOW)).toBe("just now");
    expect(formatRelativeVisitTime("not a date", NOW)).toBe("just now");
    expect(formatRelativeVisitTime("", NOW)).toBe("just now");
  });

  it("counts minutes below the hour", () => {
    expect(formatRelativeVisitTime(stamp(60_000), NOW)).toBe("1 minute ago");
    expect(formatRelativeVisitTime(stamp(59 * 60_000), NOW)).toBe("59 minutes ago");
  });

  it("counts hours below the day", () => {
    expect(formatRelativeVisitTime(stamp(60 * 60_000), NOW)).toBe("1 hour ago");
    expect(formatRelativeVisitTime(stamp(23 * 3_600_000), NOW)).toBe("23 hours ago");
  });

  it("counts days below the week", () => {
    expect(formatRelativeVisitTime(stamp(24 * 3_600_000), NOW)).toBe("1 day ago");
    expect(formatRelativeVisitTime(stamp(6 * 86_400_000), NOW)).toBe("6 days ago");
  });

  it("counts weeks below five", () => {
    expect(formatRelativeVisitTime(stamp(7 * 86_400_000), NOW)).toBe("1 week ago");
    expect(formatRelativeVisitTime(stamp(4 * 7 * 86_400_000), NOW)).toBe("4 weeks ago");
  });

  it("counts months below the year", () => {
    expect(formatRelativeVisitTime(stamp(5 * 7 * 86_400_000), NOW)).toBe("1 month ago");
    expect(formatRelativeVisitTime(stamp(11 * 30 * 86_400_000), NOW)).toBe("11 months ago");
  });

  it("counts years beyond that", () => {
    expect(formatRelativeVisitTime(stamp(12 * 30 * 86_400_000), NOW)).toBe("1 year ago");
    expect(formatRelativeVisitTime(stamp(3 * 12 * 30 * 86_400_000), NOW)).toBe("3 years ago");
  });
});
