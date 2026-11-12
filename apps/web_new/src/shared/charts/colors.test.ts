// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared chart color engine tests (parity/specs/analytics.md §10,
// NEWFRONT-732).
import { describe, expect, it } from "vitest";

import { CHART_CATEGORICAL_DARK, CHART_CATEGORICAL_LIGHT } from "@pidash/kit";

import { CHART_PALETTE_SLOTS, chartPalette, chartSlotColor, resolveChartScheme } from "./colors.js";

describe("resolveChartScheme", () => {
  it("renders dark charts for dark and dark-contrast", () => {
    expect(resolveChartScheme("dark", false)).toBe("dark");
    expect(resolveChartScheme("dark-contrast", false)).toBe("dark");
  });

  it("follows the custom palette's darkness", () => {
    expect(resolveChartScheme("custom", true)).toBe("dark");
    expect(resolveChartScheme("custom", false)).toBe("light");
  });

  it("renders light charts for light and light-contrast", () => {
    expect(resolveChartScheme("light", false)).toBe("light");
    expect(resolveChartScheme("light-contrast", false)).toBe("light");
  });
});

describe("chartSlotColor", () => {
  it("reads the first ten slots from the kit palette", () => {
    for (let index = 0; index < CHART_PALETTE_SLOTS; index += 1) {
      expect(chartSlotColor(index, "light")).toBe(CHART_CATEGORICAL_LIGHT[index]);
      expect(chartSlotColor(index, "dark")).toBe(CHART_CATEGORICAL_DARK[index]);
    }
  });

  it("extends past ten with deterministic generated hues, never cycling", () => {
    expect(chartSlotColor(10, "light")).toBe("#426ed7");
    expect(chartSlotColor(11, "light")).toBe("#d74243");
    expect(chartSlotColor(10, "dark")).toBe("#7497e7");
    expect(chartSlotColor(11, "dark")).toBe("#e77475");
    // Twice the same slot resolves identically (no randomness, no clock).
    expect(chartSlotColor(12, "light")).toBe(chartSlotColor(12, "light"));
    // The extension never repeats a kit slot.
    for (let index = 10; index < 14; index += 1) {
      expect(CHART_CATEGORICAL_LIGHT).not.toContain(chartSlotColor(index, "light"));
      expect(CHART_CATEGORICAL_DARK).not.toContain(chartSlotColor(index, "dark"));
    }
  });

  it("floors fractional indices and clamps negatives to the first slot", () => {
    expect(chartSlotColor(2.9, "light")).toBe(CHART_CATEGORICAL_LIGHT[2]);
    expect(chartSlotColor(-3, "light")).toBe(CHART_CATEGORICAL_LIGHT[0]);
    expect(chartSlotColor(-3, "dark")).toBe(CHART_CATEGORICAL_DARK[0]);
  });
});

describe("chartPalette", () => {
  it("returns the first count colors in schema order", () => {
    expect(chartPalette(3, "light")).toEqual([
      CHART_CATEGORICAL_LIGHT[0],
      CHART_CATEGORICAL_LIGHT[1],
      CHART_CATEGORICAL_LIGHT[2],
    ]);
    expect(chartPalette(12, "dark")).toHaveLength(12);
    expect(chartPalette(12, "dark")[10]).toBe("#7497e7");
  });

  it("returns no colors for non-positive counts", () => {
    expect(chartPalette(0, "light")).toEqual([]);
    expect(chartPalette(-2, "dark")).toEqual([]);
  });
});
