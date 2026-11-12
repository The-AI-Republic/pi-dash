// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { LABEL_PALETTE, randomLabelColor } from "./palette.js";

describe("LABEL_PALETTE", () => {
  it("holds the ten preset swatches", () => {
    expect([...LABEL_PALETTE]).toEqual([
      "#FF6900",
      "#FCB900",
      "#7BDCB5",
      "#00D084",
      "#8ED1FC",
      "#0693E3",
      "#ABB8C3",
      "#EB144C",
      "#F78DA7",
      "#9900EF",
    ]);
  });
});

describe("randomLabelColor", () => {
  it("maps the random source across the palette", () => {
    expect(randomLabelColor(() => 0)).toBe("#FF6900");
    expect(randomLabelColor(() => 0.35)).toBe("#00D084");
    expect(randomLabelColor(() => 0.999)).toBe("#9900EF");
  });

  it("falls back to the first swatch for an out-of-range source", () => {
    expect(randomLabelColor(() => 99)).toBe("#FF6900");
  });

  it("seeds within the palette by default", () => {
    expect(LABEL_PALETTE).toContain(randomLabelColor());
  });
});
