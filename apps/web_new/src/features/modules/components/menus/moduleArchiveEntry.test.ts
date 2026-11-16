// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail header's Archive entry (parity/specs/modules.md §4.5, §7.4,
// MOD-026): editors only, disabled with the reason until the module is
// finished, and it opens the one archive opener with the module.

import { describe, expect, it, vi } from "vitest";

import type { MenuEntry } from "@pidash/kit";

import { moduleArchiveMenuEntry } from "./moduleArchiveEntry.js";
import { MODULE_ARCHIVE_GATE_HINT } from "./moduleMenuModel.js";

const module = { id: "m1", name: "Atlas" };

/** The one action entry the helper returns (a separator carries no action). */
function onlyAction(entries: MenuEntry[]): Extract<MenuEntry, { label: string }> {
  expect(entries).toHaveLength(1);
  return entries[0] as Extract<MenuEntry, { label: string }>;
}

describe("moduleArchiveMenuEntry", () => {
  it("offers no entry to a guest", () => {
    expect(moduleArchiveMenuEntry({ status: "completed", canEdit: false, onArchive: vi.fn(), module })).toEqual([]);
  });

  it("disables the entry with the reason until the module is finished", () => {
    const entry = onlyAction(
      moduleArchiveMenuEntry({ status: "in-progress", canEdit: true, onArchive: vi.fn(), module })
    );
    expect(entry).toMatchObject({ label: "Archive", disabled: true, description: MODULE_ARCHIVE_GATE_HINT });
  });

  it.each(["completed", "cancelled"] as const)("enables it on a %s module", (status) => {
    const entry = onlyAction(moduleArchiveMenuEntry({ status, canEdit: true, onArchive: vi.fn(), module }));
    expect(entry).toMatchObject({ label: "Archive", disabled: false });
    expect(entry.description).toBeUndefined();
  });

  it("opens the archive opener with the module it names", () => {
    const onArchive = vi.fn();
    const entry = onlyAction(moduleArchiveMenuEntry({ status: "completed", canEdit: true, onArchive, module }));
    entry.onSelect();
    expect(onArchive).toHaveBeenCalledWith(module);
  });
});
