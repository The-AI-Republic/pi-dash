// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Letter and wash palette (parity/specs/pages.md §5.3, PAGE-036): the
// eight choices both rows offer, the labels each swatch and clear action
// announce, and the preview/active lookups.

import { describe, expect, it } from "vitest";

import {
  colorChoiceOf,
  colorClearLabel,
  colorPaint,
  colorSwatchActive,
  colorSwatchLabel,
  colorSwatchPaint,
  PAGE_COLOR_CHOICES,
  PAGE_COLOR_KINDS,
  PAGE_COLOR_ROW_LABELS,
} from "./pageColors.js";

describe("palette catalogue", () => {
  it("offers the eight choices in the spec's order", () => {
    expect(PAGE_COLOR_CHOICES.map((choice) => choice.key)).toEqual([
      "gray",
      "peach",
      "pink",
      "orange",
      "green",
      "light-blue",
      "dark-blue",
      "purple",
    ]);
  });

  it("names every choice", () => {
    expect(PAGE_COLOR_CHOICES.map((choice) => choice.label)).toEqual([
      "Gray",
      "Peach",
      "Pink",
      "Orange",
      "Green",
      "Light blue",
      "Dark blue",
      "Purple",
    ]);
  });

  it("paints each kind from its own token", () => {
    for (const choice of PAGE_COLOR_CHOICES) {
      expect(choice.textColor).toBe(`var(--page-color-${choice.key}-text)`);
      expect(choice.backgroundColor).toBe(`var(--page-color-${choice.key}-wash)`);
      expect(colorSwatchPaint("text", choice)).toBe(choice.textColor);
      expect(colorSwatchPaint("background", choice)).toBe(choice.backgroundColor);
    }
  });

  it("offers a letter row then a wash row", () => {
    expect(PAGE_COLOR_KINDS).toEqual(["text", "background"]);
    expect(PAGE_COLOR_ROW_LABELS.text).toBe("Letter colors");
    expect(PAGE_COLOR_ROW_LABELS.background).toBe("Wash colors");
  });
});

describe("labels", () => {
  it("names each swatch for its row", () => {
    const pink = PAGE_COLOR_CHOICES[2];
    if (pink === undefined) throw new Error("missing pink");
    expect(colorSwatchLabel("text", pink)).toBe("Letter color Pink");
    expect(colorSwatchLabel("background", pink)).toBe("Wash color Pink");
  });

  it("names each row's clear action", () => {
    expect(colorClearLabel("text")).toBe("Clear letter color");
    expect(colorClearLabel("background")).toBe("Clear wash color");
  });
});

describe("stored keys", () => {
  it("finds a known key and rejects an unknown or empty one", () => {
    expect(colorChoiceOf("green")?.label).toBe("Green");
    expect(colorChoiceOf("chartreuse")).toBeNull();
    expect(colorChoiceOf("")).toBeNull();
    expect(colorChoiceOf(null)).toBeNull();
    expect(colorChoiceOf(undefined)).toBeNull();
  });

  it("paints a known key from its token and leaves an unknown one unpainted", () => {
    expect(colorPaint("text", "pink")).toBe("var(--page-color-pink-text)");
    expect(colorPaint("background", "purple")).toBe("var(--page-color-purple-wash)");
    expect(colorPaint("text", "legacy-rgb")).toBeNull();
    expect(colorPaint("text", null)).toBeNull();
    expect(colorPaint("text", "")).toBeNull();
  });

  it("lights exactly the swatch whose key is stored", () => {
    const [gray, peach] = PAGE_COLOR_CHOICES;
    if (gray === undefined || peach === undefined) throw new Error("missing swatches");
    expect(colorSwatchActive(gray, "gray")).toBe(true);
    expect(colorSwatchActive(peach, "gray")).toBe(false);
    expect(colorSwatchActive(gray, null)).toBe(false);
  });
});
