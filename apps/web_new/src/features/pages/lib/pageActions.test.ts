// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row menu builder (parity/specs/pages.md §3.8; PAGE-010–012). The pinned
// order and the member-only duplicate entry live here so later slices add
// their owner entries to the same list.

import { describe, expect, it, vi } from "vitest";

import {
  PAGE_ACTION_COPY_LINK,
  PAGE_ACTION_DUPLICATE,
  PAGE_ACTION_OPEN_IN_NEW_TAB,
  PAGE_DUPLICATE_FAILED_TITLE,
  PAGE_DUPLICATED_TITLE,
  pageRowActions,
} from "./pageActions.js";

function handlers() {
  return { openInNewTab: vi.fn(), copyLink: vi.fn(), duplicate: vi.fn() };
}

describe("pageRowActions", () => {
  it("gives every viewer the new-tab and copy entries in the pinned order", () => {
    const calls = handlers();
    const entries = pageRowActions({ duplicate: false }, calls);
    expect(entries.map((entry) => entry.label)).toEqual([PAGE_ACTION_OPEN_IN_NEW_TAB, PAGE_ACTION_COPY_LINK]);
    expect(PAGE_ACTION_OPEN_IN_NEW_TAB).toBe("Open in new tab");
    expect(PAGE_ACTION_COPY_LINK).toBe("Copy link");
    for (const entry of entries) entry.onSelect();
    expect(calls.openInNewTab).toHaveBeenCalledTimes(1);
    expect(calls.copyLink).toHaveBeenCalledTimes(1);
    expect(calls.duplicate).not.toHaveBeenCalled();
  });

  it("appends the copy entry for a member and wires it to its handler", () => {
    const calls = handlers();
    const entries = pageRowActions({ duplicate: true }, calls);
    expect(entries.map((entry) => entry.label)).toEqual([
      PAGE_ACTION_OPEN_IN_NEW_TAB,
      PAGE_ACTION_COPY_LINK,
      PAGE_ACTION_DUPLICATE,
    ]);
    entries[2]!.onSelect();
    expect(calls.duplicate).toHaveBeenCalledTimes(1);
    expect(PAGE_ACTION_DUPLICATE).toBe("Make a copy");
  });

  it("pins the two duplicate notices", () => {
    expect(PAGE_DUPLICATED_TITLE).toBe("Page duplicated successfully.");
    expect(PAGE_DUPLICATE_FAILED_TITLE).toBe("Page could not be duplicated.");
  });
});
