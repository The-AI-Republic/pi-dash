/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { afterEach, describe, expect, it } from "vitest";

import {
  getDisplayTimeZone,
  renderFormattedDate,
  renderFormattedDateWithoutYear,
  renderFormattedPayloadDate,
  renderFormattedTime,
  setDisplayTimeZone,
} from "../src/datetime";

afterEach(() => {
  // Clear any override so tests don't leak the display timezone into each other.
  setDisplayTimeZone(undefined);
});

describe("display timezone", () => {
  it("uses an explicit override when set", () => {
    setDisplayTimeZone("Asia/Tokyo");
    expect(getDisplayTimeZone()).toBe("Asia/Tokyo");
  });

  it("ignores an invalid timezone id and falls back to detection", () => {
    setDisplayTimeZone("Not/AZone");
    // With no valid override, detection resolves to a usable id (UTC in CI/node).
    expect(getDisplayTimeZone()).toBeTruthy();
  });

  it("clearing the override re-enables detection", () => {
    setDisplayTimeZone("Europe/Paris");
    expect(getDisplayTimeZone()).toBe("Europe/Paris");
    setDisplayTimeZone(undefined);
    expect(getDisplayTimeZone()).not.toBe("Europe/Paris");
  });
});

describe("renderFormattedTime", () => {
  it("renders a UTC timestamp in the display timezone (24-hour)", () => {
    setDisplayTimeZone("Asia/Tokyo"); // UTC+9
    expect(renderFormattedTime("2025-01-15T23:30:00Z")).toBe("08:30");
  });

  it("renders in UTC when the display timezone is UTC", () => {
    setDisplayTimeZone("UTC");
    expect(renderFormattedTime("2025-01-15T23:30:00Z")).toBe("23:30");
  });

  it("supports 12-hour formatting in the display timezone", () => {
    setDisplayTimeZone("America/New_York"); // UTC-5 in January
    expect(renderFormattedTime("2025-01-15T23:30:00Z", "12-hour")).toBe("06:30 PM");
  });

  it("returns an empty string for invalid input", () => {
    expect(renderFormattedTime("not-a-date")).toBe("");
  });
});

describe("renderFormattedDate", () => {
  it("shifts the calendar date of a timestamp forward for an east-of-UTC zone", () => {
    setDisplayTimeZone("Asia/Tokyo"); // UTC+9 -> 2025-01-16 08:30 local
    expect(renderFormattedDate("2025-01-15T23:30:00Z")).toBe("Jan 16, 2025");
  });

  it("keeps the calendar date of a timestamp for a west-of-UTC zone", () => {
    setDisplayTimeZone("America/New_York"); // UTC-5 -> 2025-01-15 18:30 local
    expect(renderFormattedDate("2025-01-15T23:30:00Z")).toBe("Jan 15, 2025");
  });

  it("renders a timestamp's UTC calendar date when the display timezone is UTC", () => {
    setDisplayTimeZone("UTC");
    expect(renderFormattedDate("2025-01-15T23:30:00Z")).toBe("Jan 15, 2025");
  });

  it("does NOT shift a date-only string regardless of timezone", () => {
    setDisplayTimeZone("Asia/Tokyo");
    expect(renderFormattedDate("2025-01-15")).toBe("Jan 15, 2025");
    setDisplayTimeZone("America/New_York");
    expect(renderFormattedDate("2025-01-15")).toBe("Jan 15, 2025");
  });

  it("respects a custom format token", () => {
    setDisplayTimeZone("UTC");
    expect(renderFormattedDate("2025-01-15T23:30:00Z", "yyyy-MM-dd")).toBe("2025-01-15");
  });
});

describe("renderFormattedDateWithoutYear", () => {
  it("shifts a timestamp's date into the display timezone", () => {
    setDisplayTimeZone("Asia/Tokyo");
    expect(renderFormattedDateWithoutYear("2025-01-15T23:30:00Z")).toBe("Jan 16");
  });

  it("does not shift a date-only string", () => {
    setDisplayTimeZone("Asia/Tokyo");
    expect(renderFormattedDateWithoutYear("2025-01-15")).toBe("Jan 15");
  });
});

describe("storage layer is untouched", () => {
  it("renderFormattedPayloadDate keeps the UTC date component", () => {
    setDisplayTimeZone("Asia/Tokyo");
    // Payload date is date-only and must never be shifted by the display timezone.
    expect(renderFormattedPayloadDate("2025-01-15")).toBe("2025-01-15");
  });
});
