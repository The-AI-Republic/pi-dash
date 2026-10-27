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

  it("skips tiles without a day attribute and falls through to rects", () => {
    document.body.innerHTML = "";
    const nameless = document.createElement("div");
    nameless.setAttribute("data-calendar-day", "");
    nameless.getBoundingClientRect = () =>
      ({ left: 0, top: 0, right: 100, bottom: 100, width: 100, height: 100, x: 0, y: 0 }) as DOMRect;
    document.body.append(nameless);
    tile("2026-10-16", { left: 100, top: 0, right: 200, bottom: 100 });
    const doc = { querySelectorAll: document.querySelectorAll.bind(document) };
    expect(calendarDayAtPoint(doc, 150, 50)).toBe("2026-10-16");
  });

  it("ignores the hover preview card and resolves the tile under it (ISS-024)", () => {
    document.body.innerHTML = "";
    const source = tile("2026-10-02", { left: 0, top: 0, right: 100, bottom: 100 });
    tile("2026-10-03", { left: 100, top: 0, right: 200, bottom: 100 });
    const preview = document.createElement("div");
    preview.setAttribute("data-calendar-preview", "true");
    const inner = document.createElement("span");
    inner.textContent = "PAR-1";
    preview.append(inner);
    source.append(preview);
    const doc = {
      elementFromPoint: () => inner,
      querySelectorAll: document.querySelectorAll.bind(document),
    };
    // Release over the adjacent tile's center, where the source tile's
    // overflowed preview card sits: must resolve the destination day, not
    // the source day the card is mounted in.
    expect(calendarDayAtPoint(doc, 150, 50)).toBe("2026-10-03");
    // The same preview over its own tile still resolves the source day, so
    // the same-day no-op is preserved.
    expect(calendarDayAtPoint(doc, 50, 50)).toBe("2026-10-02");
  });

  it("still resolves direct tile hits through elementFromPoint", () => {
    document.body.innerHTML = "";
    const dest = tile("2026-10-03", { left: 100, top: 0, right: 200, bottom: 100 });
    const inner = document.createElement("span");
    dest.append(inner);
    const direct = {
      elementFromPoint: () => inner,
      querySelectorAll: document.querySelectorAll.bind(document),
    };
    expect(calendarDayAtPoint(direct, 150, 50)).toBe("2026-10-03");
    const missed = {
      elementFromPoint: () => null,
      querySelectorAll: document.querySelectorAll.bind(document),
    };
    expect(calendarDayAtPoint(missed, 150, 50)).toBe("2026-10-03");
    expect(calendarDayAtPoint(missed, 300, 50)).toBeNull();
  });
});
