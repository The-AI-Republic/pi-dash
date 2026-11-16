// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { parseViewLogo } from "./viewLogo.js";

describe("parseViewLogo", () => {
  it("reads an emoji mark", () => {
    expect(parseViewLogo({ in_use: "emoji", emoji: { value: "🚀" } })).toEqual({
      in_use: "emoji",
      emoji: { value: "🚀" },
    });
  });

  it("reads a glyph mark", () => {
    expect(parseViewLogo({ in_use: "icon", icon: { name: "Flag", color: "#5b5bd6" } })).toEqual({
      in_use: "icon",
      icon: { name: "Flag", color: "#5b5bd6" },
    });
  });

  it("reads no logo from empty or foreign shapes", () => {
    expect(parseViewLogo({})).toBeNull();
    expect(parseViewLogo(null)).toBeNull();
    expect(parseViewLogo(undefined)).toBeNull();
    expect(parseViewLogo("emoji")).toBeNull();
    expect(parseViewLogo([])).toBeNull();
    expect(parseViewLogo({ in_use: "sticker" })).toBeNull();
    expect(parseViewLogo({ in_use: "emoji" })).toBeNull();
    expect(parseViewLogo({ in_use: "emoji", emoji: { value: "" } })).toBeNull();
    expect(parseViewLogo({ in_use: "emoji", emoji: { value: 7 } })).toBeNull();
    expect(parseViewLogo({ in_use: "icon" })).toBeNull();
    expect(parseViewLogo({ in_use: "icon", icon: { name: "", color: "#5b5bd6" } })).toBeNull();
    expect(parseViewLogo({ in_use: "icon", icon: { name: "Flag", color: "" } })).toBeNull();
    expect(parseViewLogo({ in_use: "icon", icon: { name: "Flag", color: 3 } })).toBeNull();
  });
});
