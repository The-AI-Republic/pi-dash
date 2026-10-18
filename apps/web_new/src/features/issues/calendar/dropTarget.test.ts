// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { calendarDayAtPoint } from "./dropTarget.js";

function tile(iso: string, rect: { left: number; top: number; right: number; bottom: number }): HTMLElement {
  const node = document.createElement("div");
  node.setAttribute("data-calendar-day", iso);
  node.getBoundingClientRect = () =>
    ({ ...rect, width: rect.right - rect.left, height: rect.bottom - rect.top, x: rect.left, y: rect.top }) as DOMRect;
  document.body.append(node);
  return node;
}

describe("calendarDayAtPoint", () => {
  it("resolves a tile from elementFromPoint", () => {
    const node = tile("2026-10-15", { left: 0, top: 0, right: 100, bottom: 100 });
    const inner = document.createElement("span");
    node.append(inner);
    expect(calendarDayAtPoint(document, 50, 50)).toBe("2026-10-15");
  });

  it("falls back to tile rectangles when hit testing is unavailable", () => {
    document.body.innerHTML = "";
    tile("2026-10-15", { left: 0, top: 0, right: 100, bottom: 100 });
    tile("2026-10-16", { left: 100, top: 0, right: 200, bottom: 100 });
    const doc = { querySelectorAll: document.querySelectorAll.bind(document) };
    expect(calendarDayAtPoint(doc, 150, 50)).toBe("2026-10-16");
    expect(calendarDayAtPoint(doc, 300, 50)).toBeNull();
  });

  it("returns null for missing documents and invalid coordinates", () => {
    expect(calendarDayAtPoint(null, 10, 10)).toBeNull();
    expect(calendarDayAtPoint(document, Number.NaN, 10)).toBeNull();
  });

  it("ignores a hit outside any day tile", () => {
    document.body.innerHTML = "";
    expect(calendarDayAtPoint(document, 10, 10)).toBeNull();
  });
});
