// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { MenuEntry } from "@pidash/kit";
import { describe, expect, it } from "vitest";

import { COPY_WORK_ITEM_LINK_LABEL } from "../../../shared/shell/CopyWorkItemLinkButton.js";
import { intakeMobileMenuItems, type IntakeMobileMenuInput } from "./intakeMobileMenuItems.js";
import { OPEN_WORK_ITEM_LABEL } from "./IntakeDetailHeader.js";

function input(overrides: Partial<IntakeMobileMenuInput> = {}): IntakeMobileMenuInput {
  return {
    resolved: false,
    showTriage: true,
    showDelete: true,
    snoozed: false,
    onSnooze: () => undefined,
    onUnsnooze: () => undefined,
    onDuplicate: () => undefined,
    onAccept: () => undefined,
    onDecline: () => undefined,
    onDelete: () => undefined,
    onCopyLink: () => undefined,
    onOpenWorkItem: () => undefined,
    ...overrides,
  };
}

function labels(entries: MenuEntry[]): string[] {
  return entries.map((entry) => ("label" in entry ? entry.label : ""));
}

function select(entries: MenuEntry[], label: string): void {
  const entry = entries.find((candidate) => "label" in candidate && candidate.label === label);
  if (entry === undefined || !("onSelect" in entry)) throw new Error(`missing menu entry ${label}`);
  entry.onSelect();
}

describe("intakeMobileMenuItems", () => {
  it("carries every triage action with delete and the link on an open request", () => {
    const seen: string[] = [];
    const entries = intakeMobileMenuItems(
      input({
        onSnooze: () => seen.push("snooze"),
        onDuplicate: () => seen.push("duplicate"),
        onAccept: () => seen.push("accept"),
        onDecline: () => seen.push("decline"),
        onDelete: () => seen.push("delete"),
        onCopyLink: () => seen.push("copy"),
      })
    );
    expect(labels(entries)).toEqual([
      "Snooze",
      "Mark as duplicate",
      "Accept",
      "Decline",
      "Delete",
      COPY_WORK_ITEM_LINK_LABEL,
    ]);
    for (const label of labels(entries)) select(entries, label);
    expect(seen).toEqual(["snooze", "duplicate", "accept", "decline", "delete", "copy"]);
  });

  it("swaps snooze for un-snooze and drops delete without the standing", () => {
    const entries = intakeMobileMenuItems(input({ snoozed: true, showDelete: false }));
    expect(labels(entries)).toEqual(["Un snooze", "Mark as duplicate", "Accept", "Decline", COPY_WORK_ITEM_LINK_LABEL]);
  });

  it("opens empty for a viewer outside the triage gate", () => {
    expect(intakeMobileMenuItems(input({ showTriage: false }))).toEqual([]);
  });

  it("carries only the link and open entries on a resolved request", () => {
    const seen: string[] = [];
    const entries = intakeMobileMenuItems(
      input({
        resolved: true,
        showTriage: false,
        showDelete: false,
        onCopyLink: () => seen.push("copy"),
        onOpenWorkItem: () => seen.push("open"),
      })
    );
    expect(labels(entries)).toEqual([COPY_WORK_ITEM_LINK_LABEL, OPEN_WORK_ITEM_LABEL]);
    select(entries, COPY_WORK_ITEM_LINK_LABEL);
    select(entries, OPEN_WORK_ITEM_LABEL);
    expect(seen).toEqual(["copy", "open"]);
  });
});
