// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the nested-page row selector (NEWFRONT-926, PAGE-085/090).
// A tree line renders a row inside its tree item and a flat search/filter
// result renders the row alone, so the selector must answer one element per
// page. Pinned here because a double count made every tree assertion read
// each page twice on the live stack.
// @vitest-environment jsdom
import { describe, expect, it } from "vitest";

import { PAGES_NESTED_ROW_SELECTOR } from "./web-new";

function mount(html: string): void {
  document.body.innerHTML = html;
}

describe("nested page rows", () => {
  it("counts one element for a tree line and one for a flat row", () => {
    mount(`
      <div data-testid="pages-list">
        <div data-testid="page-tree-item" data-page-id="a">
          <div data-testid="page-row" data-page-id="a"><a aria-label="Alpha"></a></div>
        </div>
        <div data-testid="page-row" data-page-id="b"><a aria-label="Beta"></a></div>
      </div>
    `);
    const rows = document.querySelectorAll(PAGES_NESTED_ROW_SELECTOR);
    expect(rows).toHaveLength(2);
    expect([...rows].map((row) => row.getAttribute("data-page-id"))).toEqual(["a", "b"]);
  });

  it("does not double-count the row wrapped inside a tree item", () => {
    mount(`
      <div data-testid="pages-list">
        <div data-testid="page-tree-item" data-page-id="a">
          <div data-testid="page-row" data-page-id="a"></div>
        </div>
      </div>
    `);
    // The tree item is the line; the row it wraps is not a direct child of
    // the list, so it never matches the second half.
    expect(document.querySelectorAll(PAGES_NESTED_ROW_SELECTOR)).toHaveLength(1);
  });

  it("still finds a row that only the flat half names", () => {
    mount(`
      <div data-testid="pages-list">
        <div data-testid="page-row" data-page-id="only"></div>
      </div>
    `);
    expect(document.querySelectorAll(PAGES_NESTED_ROW_SELECTOR)).toHaveLength(1);
  });
});
