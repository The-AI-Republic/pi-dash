// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Paid-plan free-quota reminder gate (NEWPIDASH-181): paid-plan check,
// exhausted check, UTC day key, and the once-per-day storage gate.
import { describe, expect, it } from "vitest";

import {
  hasShownToday,
  isFreeAllowanceExhausted,
  isPaidPlan,
  markShownToday,
  shouldShowFreeQuotaReminder,
  utcDayKey,
  type ReminderStorage,
} from "./freeQuotaReminder.js";

function fakeStorage(initial: Record<string, string> = {}): ReminderStorage {
  const bag: Record<string, string> = { ...initial };
  return {
    getItem: (key: string): string | null => (key in bag ? (bag[key] as string) : null),
    setItem: (key: string, value: string): void => {
      bag[key] = value;
    },
  };
}

describe("isPaidPlan", () => {
  it("accepts payg and member, including raw Hub spellings", () => {
    expect(isPaidPlan("payg")).toBe(true);
    expect(isPaidPlan("member")).toBe(true);
    expect(isPaidPlan("PAYG")).toBe(true);
    expect(isPaidPlan("member+payg")).toBe(true);
    expect(isPaidPlan(5)).toBe(true);
    expect(isPaidPlan(6)).toBe(true);
  });

  it("declines free, unknown, and missing plans", () => {
    expect(isPaidPlan("free")).toBe(false);
    expect(isPaidPlan("")).toBe(false);
    expect(isPaidPlan(null)).toBe(false);
    expect(isPaidPlan(undefined)).toBe(false);
    expect(isPaidPlan(0)).toBe(false);
  });
});

describe("isFreeAllowanceExhausted", () => {
  it("reports exhausted at or above the limit", () => {
    expect(isFreeAllowanceExhausted(5_000_000, 5_000_000)).toBe(true);
    expect(isFreeAllowanceExhausted(5_000_001, 5_000_000)).toBe(true);
    expect(isFreeAllowanceExhausted(4_999_999, 5_000_000)).toBe(false);
  });

  it("never reports exhausted without two finite counts", () => {
    expect(isFreeAllowanceExhausted(null, 5_000_000)).toBe(false);
    expect(isFreeAllowanceExhausted(1, null)).toBe(false);
    expect(isFreeAllowanceExhausted(1, 0)).toBe(false);
    expect(isFreeAllowanceExhausted(1, -1)).toBe(false);
    expect(isFreeAllowanceExhausted(Number.NaN, 5)).toBe(false);
  });
});

describe("utcDayKey", () => {
  it("takes the UTC date, not the local one", () => {
    expect(utcDayKey(new Date("2026-11-10T23:59:59Z"))).toBe("2026-11-10");
    expect(utcDayKey(new Date("2026-11-11T00:00:01Z"))).toBe("2026-11-11");
  });
});

describe("once-per-day storage", () => {
  it("marks and reads the day key", () => {
    const storage = fakeStorage();
    expect(hasShownToday(storage, "2026-11-10")).toBe(false);
    markShownToday(storage, "2026-11-10");
    expect(hasShownToday(storage, "2026-11-10")).toBe(true);
    expect(hasShownToday(storage, "2026-11-11")).toBe(false);
  });

  it("reads as unshown without storage", () => {
    expect(hasShownToday(null, "2026-11-10")).toBe(false);
  });
});

describe("shouldShowFreeQuotaReminder", () => {
  const base = {
    enabled: true,
    plan: "payg",
    used: 5_000_000,
    limit: 5_000_000,
    storage: fakeStorage(),
    dayKey: "2026-11-10",
  };

  it("shows when on, paid, and exhausted", () => {
    expect(shouldShowFreeQuotaReminder(base)).toBe(true);
  });

  it("stays quiet when off, free-plan, under the limit, or already shown", () => {
    expect(shouldShowFreeQuotaReminder({ ...base, enabled: false })).toBe(false);
    expect(shouldShowFreeQuotaReminder({ ...base, plan: "free" })).toBe(false);
    expect(shouldShowFreeQuotaReminder({ ...base, used: 1 })).toBe(false);
    expect(
      shouldShowFreeQuotaReminder({
        ...base,
        storage: fakeStorage({ "pi-dash:free-quota-reminder:shown:2026-11-10": "1" }),
      })
    ).toBe(false);
  });

  it("re-fires after the UTC reset when the allowance runs out again", () => {
    const storage = fakeStorage({ "pi-dash:free-quota-reminder:shown:2026-11-10": "1" });
    expect(shouldShowFreeQuotaReminder({ ...base, storage, dayKey: "2026-11-11" })).toBe(true);
  });
});
