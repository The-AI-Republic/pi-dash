// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { previewPlacement, previewPlacementClasses } from "./placement.js";

const viewport = { width: 1000, height: 800 };

describe("calendar preview placement", () => {
  it("opens below and aligns to the start when there is room", () => {
    expect(previewPlacement({ top: 100, bottom: 130, left: 100, right: 200 }, viewport)).toBe("bottom-start");
    expect(previewPlacementClasses("bottom-start")).toBe("top-full left-0");
  });

  it("flips above near the bottom edge", () => {
    expect(previewPlacement({ top: 700, bottom: 760, left: 100, right: 200 }, viewport)).toBe("top-start");
    expect(previewPlacementClasses("top-start")).toBe("bottom-full left-0");
  });

  it("aligns to the end near the right edge", () => {
    expect(previewPlacement({ top: 100, bottom: 130, left: 800, right: 900 }, viewport)).toBe("bottom-end");
    expect(previewPlacementClasses("bottom-end")).toBe("top-full right-0");
  });

  it("flips both axes when needed", () => {
    expect(previewPlacement({ top: 700, bottom: 760, left: 800, right: 900 }, viewport)).toBe("top-end");
    expect(previewPlacementClasses("top-end")).toBe("bottom-full right-0");
  });
});
