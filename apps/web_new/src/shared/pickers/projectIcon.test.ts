// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { decodeEmojiValue, projectMark, projectMarkText } from "./projectIcon.js";

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
    [{ in_use: "icon", icon: { name: "  " } }],
    [{ in_use: "emoji", emoji: { value: "  " } }],
    [{ in_use: "emoji", emoji: { value: 3 } }],
    [{ in_use: "emoji", emoji: [] }],
  ])("falls back to the initial for %j", (logo) => {
    expect(projectMark(logo)).toEqual({ kind: "initial" });
  });

  it("reads any named icon; the glyph set decides what is drawable", () => {
    expect(projectMark({ in_use: "icon", icon: { name: "no_such_material_symbol" } })).toEqual({
      kind: "icon",
      name: "no_such_material_symbol",
      color: null,
    });
  });

  it("prefers the emoji when the stored value names both", () => {
    expect(projectMark({ in_use: "emoji", emoji: { value: "🛰️" }, icon: { name: "star" } })).toEqual({
      kind: "emoji",
      glyph: "🛰️",
    });
  });

  it("reads a stored upload's URL as an image mark, ahead of any glyph", () => {
    expect(projectMark({ in_use: "emoji", emoji: { url: "/api/assets/v2/static/emblem/", value: "🌊" } })).toEqual({
      kind: "image",
      url: "/api/assets/v2/static/emblem/",
    });
  });

  it("reads the upload record the General emblem control writes", () => {
    expect(projectMark({ in_use: "upload", url: "/api/assets/v2/static/emblem/" })).toEqual({
      kind: "image",
      url: "/api/assets/v2/static/emblem/",
    });
    expect(projectMark({ in_use: "upload", url: "  " })).toEqual({ kind: "initial" });
  });

  it("ignores a blank upload URL and falls back to the glyph", () => {
    expect(projectMark({ in_use: "emoji", emoji: { url: "  ", value: "🌊" } })).toEqual({
      kind: "emoji",
      glyph: "🌊",
    });
  });

  it("decodes a stored decimal-codepoint emoji", () => {
    expect(projectMark({ in_use: "emoji", emoji: { value: "128640" } })).toEqual({
      kind: "emoji",
      glyph: "🚀",
    });
    expect(projectMark({ in_use: "emoji", emoji: { value: "128752-65039" } })).toEqual({
      kind: "emoji",
      glyph: "🛰️",
    });
  });

  it("decodes the contract's hexadecimal page mark", () => {
    expect(projectMark({ in_use: "emoji", emoji: { value: "1f600" } })).toEqual({
      kind: "emoji",
      glyph: "😀",
    });
  });
});

describe("decodeEmojiValue", () => {
  it("passes a literal glyph through", () => {
    expect(decodeEmojiValue("🌱")).toBe("🌱");
    expect(decodeEmojiValue("AB")).toBe("AB");
  });

  it("decodes one or more code points", () => {
    expect(decodeEmojiValue("127793")).toBe("🌱");
    expect(decodeEmojiValue("128640-8205-128752")).toBe("🚀‍🛰");
    expect(decodeEmojiValue("1f600")).toBe("😀");
    expect(decodeEmojiValue("1f680-200d-1f6f0")).toBe("🚀‍🛰");
  });

  it("leaves a value that cannot decode untouched", () => {
    expect(decodeEmojiValue("1114112")).toBe("1114112");
    expect(decodeEmojiValue("12-")).toBe("12-");
    expect(decodeEmojiValue("  7")).toBe("  7");
  });

  it("leaves a surrogate alone rather than emitting a broken character", () => {
    expect(decodeEmojiValue("55296")).toBe("55296");
  });

  it("leaves an absurdly long list alone instead of overflowing", () => {
    const long = Array(300_000).fill("65").join("-");
    expect(decodeEmojiValue(long)).toBe(long);
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
