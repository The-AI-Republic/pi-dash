// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail header's read-only entries: the link actions in row
// order, each calling the shared helper for the shown module.

import { describe, expect, it, vi } from "vitest";

const share = vi.hoisted(() => ({ copyModuleLink: vi.fn(), openModuleInNewTab: vi.fn() }));
vi.mock("../../lib/moduleShare.js", () => ({
  copyModuleLink: share.copyModuleLink,
  openModuleInNewTab: share.openModuleInNewTab,
}));

import { MODULE_MENU_COPY, MODULE_MENU_OPEN } from "./moduleMenuModel.js";
import { moduleDetailMenuEntries } from "./moduleDetailMenuEntries.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

describe("module detail menu entries", () => {
  it("offers open in a new tab before copy link", () => {
    const entries = moduleDetailMenuEntries(WS, PROJECT, MODULE);
    expect(entries.map((entry) => ("label" in entry ? entry.label : null))).toEqual([
      MODULE_MENU_OPEN,
      MODULE_MENU_COPY,
    ]);
  });

  it("opens the shown module in a new tab", () => {
    const entries = moduleDetailMenuEntries(WS, PROJECT, MODULE);
    const open = entries[0];
    expect(open !== undefined && "onSelect" in open).toBe(true);
    if (open !== undefined && "onSelect" in open) open.onSelect();
    expect(share.openModuleInNewTab).toHaveBeenCalledWith(WS, PROJECT, MODULE);
  });

  it("copies the shown module's link", () => {
    const entries = moduleDetailMenuEntries(WS, PROJECT, MODULE);
    const copy = entries[1];
    expect(copy !== undefined && "onSelect" in copy).toBe(true);
    if (copy !== undefined && "onSelect" in copy) copy.onSelect();
    expect(share.copyModuleLink).toHaveBeenCalledWith(WS, PROJECT, MODULE);
  });
});
