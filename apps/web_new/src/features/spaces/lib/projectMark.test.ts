// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { projectMarkGlyph } from "./projectMark.js";

describe("projectMarkGlyph", () => {
  it("decodes an emoji mark", () => {
    expect(projectMarkGlyph({ in_use: "emoji", emoji: { value: "128512" } })).toBe("😀");
    expect(projectMarkGlyph({ in_use: "emoji", emoji: { value: 128512 } })).toBe("😀");
  });

  it("returns null for icon marks, absent emoji and non-string values", () => {
    expect(projectMarkGlyph({ in_use: "icon", icon: { name: "rocket" } })).toBeNull();
    expect(projectMarkGlyph({ in_use: "emoji" })).toBeNull();
    expect(projectMarkGlyph({ in_use: "emoji", emoji: null })).toBeNull();
    expect(projectMarkGlyph({ in_use: "emoji", emoji: { value: {} } })).toBeNull();
    expect(projectMarkGlyph({})).toBeNull();
  });

  it("throws on an invalid code point so the error boundary can catch it", () => {
    expect(() => projectMarkGlyph({ in_use: "emoji", emoji: { value: "not-a-code-point" } })).toThrow(RangeError);
    expect(() => projectMarkGlyph({ in_use: "emoji", emoji: { value: "9999999" } })).toThrow(RangeError);
    expect(() => projectMarkGlyph({ in_use: "emoji", emoji: { value: "-1" } })).toThrow(RangeError);
  });
});
