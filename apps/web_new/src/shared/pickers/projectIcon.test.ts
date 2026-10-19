// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { projectIconGlyph, projectMark, projectMarkText } from "./projectIcon.js";

describe("projectMark", () => {
  it("reads the picked emoji", () => {
    expect(projectMark({ in_use: "emoji", emoji: { value: "🌱" } })).toEqual({ kind: "emoji", glyph: "🌱" });
  });

  it("reads a configured icon and its color", () => {
    expect(projectMark({ in_use: "icon", icon: { name: "rocket_launch", color: "#f97316" } })).toEqual({
      kind: "icon",
      name: "rocket_launch",
      color: "#f97316",
    });
    expect(projectMark({ in_use: "icon", icon: { name: "star", color: "" } })).toEqual({
      kind: "icon",
      name: "star",
      color: null,
    });
  });

  it.each([
    [{}],
    [{ in_use: "unknown" }],
    [{ in_use: "icon" }],
    [{ in_use: "icon", icon: null }],
    [{ in_use: "icon", icon: { name: 7 } }],
    [{ in_use: "icon", icon: { name: "no_such_material_symbol" } }],
    [{ in_use: "emoji", emoji: { value: "  " } }],
    [{ in_use: "emoji", emoji: { value: 3 } }],
    [{ in_use: "emoji", emoji: [] }],
  ])("falls back to the initial for %j", (logo) => {
    expect(projectMark(logo)).toEqual({ kind: "initial" });
  });

  it("prefers the emoji when the stored value names both", () => {
    expect(projectMark({ in_use: "emoji", emoji: { value: "🛰️" }, icon: { name: "star" } })).toEqual({
      kind: "emoji",
      glyph: "🛰️",
    });
  });
});

describe("projectIconGlyph", () => {
  it("maps a stored Material name to a glyph", () => {
    expect(projectIconGlyph("rocket_launch")).not.toBeNull();
    expect(projectIconGlyph("star")).not.toBeNull();
  });

  it("has no glyph for a name outside the set", () => {
    expect(projectIconGlyph("made_up_symbol")).toBeNull();
    expect(projectIconGlyph("")).toBeNull();
  });
});

describe("projectMarkText", () => {
  it("reads the emoji or the identifier's initial", () => {
    expect(projectMarkText({ kind: "emoji", glyph: "🌱" }, "SEED")).toBe("🌱");
    expect(projectMarkText({ kind: "icon", name: "star", color: null }, "seed")).toBe("S");
    expect(projectMarkText({ kind: "initial" }, "  seed")).toBe("S");
    expect(projectMarkText({ kind: "initial" }, "")).toBe("?");
  });
});
