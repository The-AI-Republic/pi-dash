// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference row vocabulary (parity/specs/profile.md §7, PROF-016/017):
// the option tables, the zero-based week-start index and the labels each
// row shows for its stored value.
import { describe, expect, it } from "vitest";

import {
  languageLabel,
  normalizeWeekStart,
  THEME_SELECT_OPTIONS,
  weekStartLabel,
  WEEK_START_OPTIONS,
  WEEKDAY_LABELS,
} from "./choices.js";

describe("preference choices", () => {
  it("offers every theme in engine order", () => {
    expect(THEME_SELECT_OPTIONS.map((option) => option.value)).toEqual([
      "system",
      "light",
      "dark",
      "light-contrast",
      "dark-contrast",
      "custom",
    ]);
    expect(THEME_SELECT_OPTIONS.map((option) => option.label)).toEqual([
      "System preference",
      "Light",
      "Dark",
      "Light high contrast",
      "Dark high contrast",
      "Custom theme",
    ]);
  });

  it("offers Sunday at zero through Saturday", () => {
    expect(WEEKDAY_LABELS).toHaveLength(7);
    expect(WEEK_START_OPTIONS.map((option) => option.value)).toEqual([0, 1, 2, 3, 4, 5, 6]);
    expect(WEEK_START_OPTIONS.map((option) => option.label)).toEqual([
      "Sunday",
      "Monday",
      "Tuesday",
      "Wednesday",
      "Thursday",
      "Friday",
      "Saturday",
    ]);
  });

  it("normalizes only integer weekday indexes and falls back to Sunday", () => {
    expect(normalizeWeekStart(3)).toBe(3);
    expect(normalizeWeekStart(0)).toBe(0);
    expect(normalizeWeekStart(6)).toBe(6);
    expect(normalizeWeekStart(7)).toBe(0);
    expect(normalizeWeekStart(-1)).toBe(0);
    expect(normalizeWeekStart(1.5)).toBe(0);
    expect(normalizeWeekStart(undefined)).toBe(0);
    expect(normalizeWeekStart(null)).toBe(0);
  });

  it("labels the stored week start", () => {
    expect(weekStartLabel(1)).toBe("Monday");
    expect(weekStartLabel(6)).toBe("Saturday");
    expect(weekStartLabel(9)).toBe("Sunday");
    expect(weekStartLabel(undefined)).toBe("Sunday");
  });

  it("labels a shipped language and falls back to the raw tag", () => {
    const supported = [
      { tag: "en", label: "English" },
      { tag: "fr", label: "Français" },
    ];
    expect(languageLabel("fr", supported)).toBe("Français");
    expect(languageLabel("de", supported)).toBe("de");
    expect(languageLabel("", supported)).toBe("");
  });
});
