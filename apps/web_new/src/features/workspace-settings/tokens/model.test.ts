// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  backupFileName,
  earliestCustomDay,
  emptyTokenDraft,
  expiredAgoLine,
  expiryLine,
  isExpiredToken,
  localIsoDay,
  resolveExpiry,
  revealExpiryLine,
  rowDescription,
  rowExpiryLine,
  titleError,
  TITLE_REQUIRED_ERROR,
  TITLE_TOO_LONG_ERROR,
  tokenBackupCsv,
  type TokenDraft,
} from "./model.js";

const NOW = new Date(2026, 9, 19, 14, 5, 30, 250);

function draft(patch: Partial<TokenDraft>): TokenDraft {
  return { ...emptyTokenDraft(), ...patch };
}

describe("title validation", () => {
  it("requires a non-blank title", () => {
    expect(titleError("")).toBe(TITLE_REQUIRED_ERROR);
    expect(titleError("   ")).toBe(TITLE_REQUIRED_ERROR);
  });

  it("caps the trimmed title at 255 characters", () => {
    expect(titleError("x".repeat(255))).toBeNull();
    expect(titleError("  " + "x".repeat(255) + "  ")).toBeNull();
    expect(titleError("x".repeat(256))).toBe(TITLE_TOO_LONG_ERROR);
  });
});

describe("expiry resolution", () => {
  it("never expires when the toggle is on, regardless of a picked choice", () => {
    expect(resolveExpiry(draft({ neverExpires: true, choice: "week" }), NOW)).toBeNull();
  });

  it("requires a date when nothing is picked", () => {
    expect(resolveExpiry(emptyTokenDraft(), NOW)).toBeUndefined();
  });

  it("counts each preset forward from now", () => {
    expect(resolveExpiry(draft({ choice: "week" }), NOW)).toBe("2026-10-26T21:05:30.250Z");
    const month = new Date(NOW.getTime());
    month.setMonth(month.getMonth() + 1);
    expect(resolveExpiry(draft({ choice: "month" }), NOW)).toBe(month.toISOString());
    const quarter = new Date(NOW.getTime());
    quarter.setMonth(quarter.getMonth() + 3);
    expect(resolveExpiry(draft({ choice: "quarter" }), NOW)).toBe(quarter.toISOString());
    expect(resolveExpiry(draft({ choice: "year" }), NOW)).toBe("2027-10-19T21:05:30.250Z");
  });

  it("requires a custom day once Custom is picked", () => {
    expect(resolveExpiry(draft({ choice: "custom" }), NOW)).toBeUndefined();
  });

  it("clamps month presets to the last day of a shorter target month", () => {
    const jan31 = new Date(2026, 0, 31, 9, 15);
    const oneMonth = new Date(2026, 0, 31, 9, 15);
    oneMonth.setMonth(oneMonth.getMonth() + 1);
    oneMonth.setDate(0);
    expect(resolveExpiry(draft({ choice: "month" }), jan31)).toBe(oneMonth.toISOString());

    const may31 = new Date(2026, 4, 31, 9, 15);
    expect(resolveExpiry(draft({ choice: "month" }), may31)).toBe(new Date(2026, 5, 30, 9, 15).toISOString());
    expect(resolveExpiry(draft({ choice: "quarter" }), may31)).toBe(new Date(2026, 7, 31, 9, 15).toISOString());

    const nov30 = new Date(2026, 10, 30, 9, 15);
    expect(resolveExpiry(draft({ choice: "quarter" }), nov30)).toBe(new Date(2027, 1, 28, 9, 15).toISOString());
  });

  it("keeps the current time of day for a custom day", () => {
    const expected = new Date(2026, 10, 15, NOW.getHours(), NOW.getMinutes(), NOW.getSeconds(), NOW.getMilliseconds());
    expect(resolveExpiry(draft({ choice: "custom", customDay: "2026-11-15" }), NOW)).toBe(expected.toISOString());
  });
});

describe("calendar floor", () => {
  it("formats local days without UTC drift", () => {
    expect(localIsoDay(new Date(2026, 0, 2, 23, 59))).toBe("2026-01-02");
  });

  it("floors custom days at tomorrow, including a month/year rollover", () => {
    expect(earliestCustomDay(new Date(2026, 9, 19, 23, 59))).toBe("2026-10-20");
    expect(earliestCustomDay(new Date(2026, 11, 31, 12))).toBe("2027-01-01");
  });
});

describe("expiry wording", () => {
  it("renders a human line for an instant and the never fallback", () => {
    expect(expiryLine("2030-06-15T12:00:00Z")).toMatch(/^Expires Jun 15, 2030 at \d{2}:00$/);
    expect(expiryLine(null)).toBe("Never expires");
    expect(expiryLine(undefined)).toBe("Never expires");
    expect(expiryLine("not-a-date")).toBe("Never expires");
  });

  it("uses the same line for the reveal", () => {
    expect(revealExpiryLine(null)).toBe("Never expires");
    expect(revealExpiryLine("2030-06-15T12:00:00Z")).toBe(expiryLine("2030-06-15T12:00:00Z"));
  });

  it("describes how long ago an expired token lapsed", () => {
    const now = new Date("2026-10-19T12:00:00Z");
    expect(expiredAgoLine("2026-10-19T11:59:40Z", now)).toBe("Expired just now");
    expect(expiredAgoLine("2026-10-19T11:59:00Z", now)).toBe("Expired 1 minute ago");
    expect(expiredAgoLine("2026-10-19T11:58:00Z", now)).toBe("Expired 2 minutes ago");
    expect(expiredAgoLine("2026-10-19T10:00:00Z", now)).toBe("Expired 2 hours ago");
    expect(expiredAgoLine("2026-10-18T12:00:00Z", now)).toBe("Expired 1 day ago");
    expect(expiredAgoLine("2026-09-01T12:00:00Z", now)).toBe("Expired 1 month ago");
    expect(expiredAgoLine("2022-01-01T12:00:00Z", now)).toBe("Expired 4 years ago");
  });

  it("picks future, never, or relative past wording from the row", () => {
    const now = new Date("2026-10-19T12:00:00Z");
    expect(rowExpiryLine({ is_active: true, expired_at: null }, now)).toBe("Never expires");
    expect(rowExpiryLine({ is_active: true, expired_at: "2030-06-15T12:00:00Z" }, now)).toBe(
      expiryLine("2030-06-15T12:00:00Z")
    );
    expect(rowExpiryLine({ is_active: false, expired_at: "2026-10-17T12:00:00Z" }, now)).toBe("Expired 2 days ago");
    expect(isExpiredToken({ is_active: false })).toBe(true);
    expect(isExpiredToken({ is_active: true })).toBe(false);
  });

  it("hides blank descriptions only", () => {
    expect(rowDescription({ description: "  " })).toBeNull();
    expect(rowDescription({ description: "First token" })).toBe("First token");
  });
});

describe("backup file", () => {
  it("names the file with the creation timestamp", () => {
    expect(backupFileName(new Date(1760000000000))).toBe("secret-key-1760000000000.csv");
  });

  it("writes the pinned header and escapes CSV-sensitive fields", () => {
    const csv = tokenBackupCsv({
      title: "Deploy bot",
      description: 'CI, "nightly"',
      expiry: "Never expires",
      secret: "pi_dash_api_abc123",
    });
    expect(csv.split("\n")[0]).toBe("Title,Description,Expiry,Secret key");
    expect(csv).toContain('Deploy bot,"CI, ""nightly""",Never expires,pi_dash_api_abc123');
  });

  it("leaves plain cells unquoted", () => {
    expect(tokenBackupCsv({ title: " Plain ", description: "", expiry: "Expires", secret: "s" })).toBe(
      "Title,Description,Expiry,Secret key\nPlain,,Expires,s\n"
    );
  });
});
