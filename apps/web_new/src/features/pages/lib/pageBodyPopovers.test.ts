// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  PAGE_POPOVER_ANCHOR_GAP,
  PAGE_POPOVER_VIEWPORT_MARGIN,
  pagePopoverMaxSize,
  pagePopoverPlacement,
  pageTriggerReopened,
} from "./pageBodyPopovers.js";

const VIEWPORT = { width: 1024, height: 768 };

describe("pagePopoverPlacement", () => {
  it("opens just below the caret when the popover fits (PAGE-037)", () => {
    expect(pagePopoverPlacement({ top: 100, left: 40 }, { width: 256, height: 320 }, VIEWPORT)).toEqual({
      top: 100 + PAGE_POPOVER_ANCHOR_GAP,
      left: 40,
    });
  });

  it("flips above the caret when the lower edge is short (PAGE-037)", () => {
    const placement = pagePopoverPlacement({ top: 700, left: 40 }, { width: 256, height: 320 }, VIEWPORT);
    expect(placement.top).toBe(700 - PAGE_POPOVER_ANCHOR_GAP - 320);
    expect(placement.top + 320).toBeLessThanOrEqual(VIEWPORT.height);
  });

  it("clamps the box inside the viewport margin when it fits nowhere", () => {
    const placement = pagePopoverPlacement({ top: 400, left: 40 }, { width: 256, height: 900 }, VIEWPORT);
    expect(placement.top).toBe(PAGE_POPOVER_VIEWPORT_MARGIN);
  });

  it("keeps a popover left of the right edge and off the left edge", () => {
    expect(pagePopoverPlacement({ top: 10, left: 1000 }, { width: 256, height: 100 }, VIEWPORT).left).toBe(
      VIEWPORT.width - PAGE_POPOVER_VIEWPORT_MARGIN - 256
    );
    expect(pagePopoverPlacement({ top: 10, left: -20 }, { width: 256, height: 100 }, VIEWPORT).left).toBe(
      PAGE_POPOVER_VIEWPORT_MARGIN
    );
  });

  it("places below the caret before the viewport is measured", () => {
    expect(pagePopoverPlacement({ top: 60, left: 12 }, { width: 0, height: 0 }, { width: 0, height: 0 })).toEqual({
      top: 60 + PAGE_POPOVER_ANCHOR_GAP,
      left: 12,
    });
  });

  it("never places a popover above the viewport on a tiny screen", () => {
    const placement = pagePopoverPlacement(
      { top: 40, left: 8 },
      { width: 500, height: 400 },
      { width: 320, height: 300 }
    );
    expect(placement.top).toBeGreaterThanOrEqual(0);
    expect(placement.left).toBeGreaterThanOrEqual(0);
  });
});

describe("pagePopoverMaxSize", () => {
  it("keeps the viewport margin on both sides", () => {
    expect(pagePopoverMaxSize(VIEWPORT)).toEqual({
      width: VIEWPORT.width - 2 * PAGE_POPOVER_VIEWPORT_MARGIN,
      height: VIEWPORT.height - 2 * PAGE_POPOVER_VIEWPORT_MARGIN,
    });
  });

  it("never reports a negative cap", () => {
    expect(pagePopoverMaxSize({ width: 4, height: 4 })).toEqual({ width: 0, height: 0 });
  });
});

describe("pageTriggerReopened", () => {
  it("lets the trigger open when nothing was dismissed", () => {
    expect(pageTriggerReopened(null, "<p>/hea</p>")).toBe(true);
  });

  it("keeps a dismissed trigger shut until the body changes (PAGE-037)", () => {
    expect(pageTriggerReopened("<p>/hea</p>", "<p>/hea</p>")).toBe(false);
    expect(pageTriggerReopened("<p>/hea</p>", "<p>/head</p>")).toBe(true);
  });
});
