// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration session identity (parity/specs/pages.md §8.2, PAGE-066):
// the cursor colour is a deterministic hex triple of the user id, and
// the handshake token carries id, name and colour as JSON.

import { describe, expect, it } from "vitest";

import { liveCursorColor, liveHueOf, liveIdentityOf, liveIdentityToken } from "./pageLiveIdentity.js";

/** First probe id whose hue lands in [low, high). */
function hueIn(low: number, high: number): string {
  for (let index = 0; index < 5000; index += 1) {
    const id = `probe-${index}`;
    const hue = liveHueOf(id);
    if (hue >= low && hue < high) return id;
  }
  throw new Error("no probe hue in range");
}

describe("liveCursorColor", () => {
  it("paints a lowercase hex triple", () => {
    expect(liveCursorColor("user-1")).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("is deterministic for one user", () => {
    expect(liveCursorColor("user-1")).toBe(liveCursorColor("user-1"));
  });

  it("differs between people", () => {
    const first = liveCursorColor("user-1");
    let second = first;
    for (let index = 2; second === first && index < 5000; index += 1) {
      second = liveCursorColor(`user-${index}`);
    }
    expect(second).not.toBe(first);
  });

  it("covers every sector of the ring", () => {
    const sectors: Array<[number, number]> = [
      [0, 60],
      [60, 180],
      [180, 240],
      [240, 360],
    ];
    for (const [low, high] of sectors) {
      expect(liveCursorColor(hueIn(low, high))).toMatch(/^#[0-9a-f]{6}$/);
    }
  });

  it("pins the colour for a known id", () => {
    expect(liveCursorColor("423e4567-e89b-12d3-a456-426614174003")).toBe("#ba2c54");
  });
});

describe("liveHueOf", () => {
  it("lands on the ring and holds still", () => {
    const hue = liveHueOf("user-1");
    expect(hue).toBeGreaterThanOrEqual(0);
    expect(hue).toBeLessThan(360);
    expect(liveHueOf("user-1")).toBe(hue);
  });
});

describe("liveIdentityOf", () => {
  it("carries id, name and the derived colour", () => {
    expect(liveIdentityOf({ id: "user-1", name: "Ada" })).toEqual({
      id: "user-1",
      name: "Ada",
      color: liveCursorColor("user-1"),
    });
  });
});

describe("liveIdentityToken", () => {
  it("serialises the identity as JSON", () => {
    const token = liveIdentityToken({ id: "user-1", name: "Ada" });
    expect(JSON.parse(token)).toEqual({
      id: "user-1",
      name: "Ada",
      color: liveCursorColor("user-1"),
    });
  });
});
