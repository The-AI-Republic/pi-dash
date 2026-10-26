// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail options menu (parity/specs/pages.md §7.4, PAGE-060) and the
// shelved marker (spec §2.3, PAGE-059): the pinned owner order, the
// member reduction, the shelved set, the hosted entries, and the dated
// marker line.

import { describe, expect, it, vi } from "vitest";

import type { PageBodyOptionActions } from "./pageBodyActions.js";
import { PAGE_COPY_MARKDOWN_LABEL } from "./pageBodyActions.js";
import { PAGE_FULL_WIDTH_LABEL, PAGE_STICKY_TOOLBAR_LABEL } from "./pageDisplayMenu.js";
import { PAGE_ACTION_ADD_SUB_PAGE } from "./pageNested.js";
import { PAGE_PREFERENCES_DEFAULTS } from "./pagePreferences.js";
import {
  pageArchivedMarker,
  pageDetailMenuItems,
  PAGE_EXPORT_ITEM,
  PAGE_HISTORY_ITEM,
  type PageDetailMenuCapabilities,
  type PageDetailMenuHandlers,
} from "./pageDetailMenu.js";

const capabilities: PageDetailMenuCapabilities = {
  canDuplicate: true,
  canManage: true,
  archived: false,
  restricted: false,
  body: null,
};

function handlers(overrides: Partial<PageDetailMenuHandlers> = {}): PageDetailMenuHandlers {
  return {
    onOpenHistory: vi.fn(),
    onDuplicate: vi.fn(),
    onToggleArchive: vi.fn(),
    onToggleVisibility: vi.fn(),
    onRequestDelete: vi.fn(),
    ...overrides,
  };
}

function labels(items: { label: string }[]): string[] {
  return items.map((item) => item.label);
}

function bodyActions(): PageBodyOptionActions {
  return { copyMarkdownLabel: PAGE_COPY_MARKDOWN_LABEL, copyMarkdown: vi.fn(async () => undefined) };
}

describe("pageDetailMenuItems", () => {
  it("orders the owner menu exactly as the oracle pins it", () => {
    const items = pageDetailMenuItems(
      { ...capabilities, body: bodyActions() },
      handlers({ onExport: vi.fn(), onAddSubPage: vi.fn() }),
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    expect(labels(items)).toEqual([
      PAGE_FULL_WIDTH_LABEL,
      PAGE_STICKY_TOOLBAR_LABEL,
      PAGE_COPY_MARKDOWN_LABEL,
      PAGE_HISTORY_ITEM,
      PAGE_ACTION_ADD_SUB_PAGE,
      "Make a copy",
      "Archive",
      "Make private",
      PAGE_EXPORT_ITEM,
    ]);
  });

  it("drops the privileged entries for a member", () => {
    const items = pageDetailMenuItems(
      { ...capabilities, canManage: false, body: bodyActions() },
      handlers({ onExport: vi.fn() }),
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    expect(labels(items)).toEqual([
      PAGE_FULL_WIDTH_LABEL,
      PAGE_STICKY_TOOLBAR_LABEL,
      PAGE_COPY_MARKDOWN_LABEL,
      PAGE_HISTORY_ITEM,
      "Make a copy",
      PAGE_EXPORT_ITEM,
    ]);
  });

  it("hides duplication below member roles", () => {
    const items = pageDetailMenuItems(
      { ...capabilities, canDuplicate: false, canManage: false },
      handlers(),
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    expect(labels(items)).not.toContain("Make a copy");
  });

  it("swaps archive for restore plus delete while shelved, hiding visibility and the sticky toggle", () => {
    const items = pageDetailMenuItems(
      { ...capabilities, archived: true },
      handlers(),
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    expect(labels(items)).toEqual([PAGE_FULL_WIDTH_LABEL, PAGE_HISTORY_ITEM, "Make a copy", "Restore", "Delete"]);
  });

  it("offers Make public for a restricted page", () => {
    const items = pageDetailMenuItems(
      { ...capabilities, restricted: true },
      handlers(),
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    expect(labels(items)).toContain("Make public");
    expect(labels(items)).not.toContain("Make private");
  });

  it("omits the hosted entries until they are wired", () => {
    const items = pageDetailMenuItems(capabilities, handlers(), PAGE_PREFERENCES_DEFAULTS, vi.fn());
    expect(labels(items)).not.toContain(PAGE_COPY_MARKDOWN_LABEL);
    expect(labels(items)).not.toContain(PAGE_EXPORT_ITEM);
    expect(labels(items)).not.toContain(PAGE_ACTION_ADD_SUB_PAGE);
  });

  it("never carries the row menu's new-tab, copy-link, lock or move entries", () => {
    const items = pageDetailMenuItems(
      { ...capabilities, body: bodyActions(), archived: true },
      handlers({ onExport: vi.fn(), onAddSubPage: vi.fn() }),
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    const names = labels(items);
    expect(names).not.toContain("Open in new tab");
    expect(names).not.toContain("Copy link");
    expect(names).not.toContain("Lock");
    expect(names).not.toContain("Unlock");
    expect(names.some((name) => /move to project/i.test(name))).toBe(false);
  });

  it("runs each entry's handler", () => {
    const seen = handlers({ onExport: vi.fn(), onAddSubPage: vi.fn() });
    const items = pageDetailMenuItems(
      { ...capabilities, body: bodyActions() },
      seen,
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    for (const item of items) item.onSelect();
    expect(seen.onOpenHistory).toHaveBeenCalledTimes(1);
    expect(seen.onDuplicate).toHaveBeenCalledTimes(1);
    expect(seen.onToggleArchive).toHaveBeenCalledTimes(1);
    expect(seen.onToggleVisibility).toHaveBeenCalledTimes(1);
    expect(seen.onExport).toHaveBeenCalledTimes(1);
    expect(seen.onAddSubPage).toHaveBeenCalledTimes(1);
  });

  it("runs restore, delete and the hosted markup copy on a shelved editor page", async () => {
    const copyMarkdown = vi.fn(async () => undefined);
    const seen = handlers();
    const items = pageDetailMenuItems(
      {
        ...capabilities,
        archived: true,
        body: { copyMarkdownLabel: PAGE_COPY_MARKDOWN_LABEL, copyMarkdown },
      },
      seen,
      PAGE_PREFERENCES_DEFAULTS,
      vi.fn()
    );
    for (const item of items) item.onSelect();
    await vi.waitFor(() => expect(copyMarkdown).toHaveBeenCalledTimes(1));
    expect(seen.onToggleArchive).toHaveBeenCalledTimes(1);
    expect(seen.onRequestDelete).toHaveBeenCalledTimes(1);
    expect(seen.onToggleVisibility).not.toHaveBeenCalled();
  });

  it("writes the display toggles through the preference store", () => {
    const onPreferenceChange = vi.fn();
    const items = pageDetailMenuItems(capabilities, handlers(), PAGE_PREFERENCES_DEFAULTS, onPreferenceChange);
    items[0]?.onSelect();
    items[1]?.onSelect();
    expect(onPreferenceChange).toHaveBeenCalledWith({ full_width: true });
    expect(onPreferenceChange).toHaveBeenCalledWith({ sticky_toolbar: false });
  });
});

describe("pageArchivedMarker", () => {
  it("renders the pinned dated line", () => {
    expect(pageArchivedMarker("2026-03-04")).toMatch(/^Archived at .+, 2026$/);
    expect(pageArchivedMarker("2026-03-04")).toContain("Archived at ");
  });
});
