// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { resolveTabScroll, tabScrollBehavior } from "./workspaceTabStrip.js";

describe("resolveTabScroll", () => {
  it("leaves a fully visible tab where it is", () => {
    expect(resolveTabScroll({ left: 20, right: 80 }, { left: 0, right: 100 })).toEqual({ scroll: false });
    expect(resolveTabScroll({ left: 0, right: 100 }, { left: 0, right: 100 })).toEqual({ scroll: false });
  });

  it("centers a tab fully off-screen", () => {
    expect(resolveTabScroll({ left: -80, right: -20 }, { left: 0, right: 100 })).toEqual({
      scroll: true,
      inline: "center",
    });
    expect(resolveTabScroll({ left: 120, right: 180 }, { left: 0, right: 100 })).toEqual({
      scroll: true,
      inline: "center",
    });
  });

  it("takes the nearest edge for a partially visible tab", () => {
    expect(resolveTabScroll({ left: -20, right: 40 }, { left: 0, right: 100 })).toEqual({
      scroll: true,
      inline: "nearest",
    });
    expect(resolveTabScroll({ left: 60, right: 140 }, { left: 0, right: 100 })).toEqual({
      scroll: true,
      inline: "nearest",
    });
  });
});

describe("tabScrollBehavior", () => {
  it("scrolls instantly when reduced motion is preferred", () => {
    expect(tabScrollBehavior({ matches: true })).toBe("auto");
  });

  it("scrolls smoothly otherwise", () => {
    expect(tabScrollBehavior({ matches: false })).toBe("smooth");
    expect(tabScrollBehavior(null)).toBe("smooth");
  });
});
