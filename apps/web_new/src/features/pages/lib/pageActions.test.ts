// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row menu builder (parity/specs/pages.md §3.8; PAGE-010–017). The pinned
// order, the member-only duplicate entry and the owner/admin state switches
// live here so the row and the context menu never fork.

import { describe, expect, it, vi } from "vitest";

import {
  PAGE_ACTION_ARCHIVE,
  PAGE_ACTION_COPY_LINK,
  PAGE_ACTION_DELETE,
  PAGE_ACTION_DUPLICATE,
  PAGE_ACTION_LOCK,
  PAGE_ACTION_MAKE_PRIVATE,
  PAGE_ACTION_MAKE_PUBLIC,
  PAGE_ACTION_MOVE,
  PAGE_ACTION_OPEN_IN_NEW_TAB,
  PAGE_ACTION_RESTORE,
  PAGE_ACTION_UNLOCK,
  PAGE_ARCHIVED_TITLE,
  PAGE_ARCHIVE_FAILED_TITLE,
  PAGE_DELETED_TITLE,
  PAGE_DELETE_FAILED_TITLE,
  PAGE_DUPLICATE_FAILED_TITLE,
  PAGE_DUPLICATED_TITLE,
  PAGE_LOCKED_TITLE,
  PAGE_LOCK_FAILED_TITLE,
  PAGE_MOVED_TITLE,
  PAGE_MOVE_FAILED_TITLE,
  PAGE_RESTORED_TITLE,
  PAGE_RESTORE_FAILED_TITLE,
  PAGE_UNLOCKED_TITLE,
  PAGE_VISIBILITY_FAILED_TITLE,
  PAGE_VISIBILITY_PUBLIC_SUFFIX,
  PAGE_VISIBILITY_SUFFIX,
  pageRowActions,
  type PageRowCapabilities,
} from "./pageActions.js";

const VIEWER: PageRowCapabilities = {
  duplicate: false,
  manage: false,
  archived: false,
  locked: false,
  restricted: false,
  move: false,
};
const MEMBER: PageRowCapabilities = { ...VIEWER, duplicate: true };
const OWNER: PageRowCapabilities = { ...MEMBER, manage: true };

function handlers() {
  return {
    openInNewTab: vi.fn(),
    copyLink: vi.fn(),
    duplicate: vi.fn(),
    toggleVisibility: vi.fn(),
    toggleLock: vi.fn(),
    toggleArchive: vi.fn(),
    requestDelete: vi.fn(),
    requestMove: vi.fn(),
  };
}

function labels(capabilities: PageRowCapabilities): string[] {
  return pageRowActions(capabilities, handlers()).map((entry) => entry.label);
}

describe("pageRowActions", () => {
  it("gives every viewer the new-tab and copy entries in the pinned order", () => {
    const calls = handlers();
    const entries = pageRowActions(VIEWER, calls);
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
    const entries = pageRowActions(MEMBER, calls);
    expect(entries.map((entry) => entry.label)).toEqual([
      PAGE_ACTION_OPEN_IN_NEW_TAB,
      PAGE_ACTION_COPY_LINK,
      PAGE_ACTION_DUPLICATE,
    ]);
    entries[2]!.onSelect();
    expect(calls.duplicate).toHaveBeenCalledTimes(1);
    expect(PAGE_ACTION_DUPLICATE).toBe("Make a copy");
  });

  it("appends the owner switches in the pinned order on a live page", () => {
    const calls = handlers();
    const entries = pageRowActions(OWNER, calls);
    expect(entries.map((entry) => entry.label)).toEqual([
      PAGE_ACTION_OPEN_IN_NEW_TAB,
      PAGE_ACTION_COPY_LINK,
      PAGE_ACTION_DUPLICATE,
      PAGE_ACTION_LOCK,
      PAGE_ACTION_MAKE_PRIVATE,
      PAGE_ACTION_ARCHIVE,
    ]);
    expect(PAGE_ACTION_LOCK).toBe("Lock");
    expect(PAGE_ACTION_MAKE_PRIVATE).toBe("Make private");
    expect(PAGE_ACTION_ARCHIVE).toBe("Archive");
    entries[3]!.onSelect();
    entries[4]!.onSelect();
    entries[5]!.onSelect();
    expect(calls.toggleLock).toHaveBeenCalledTimes(1);
    expect(calls.toggleVisibility).toHaveBeenCalledTimes(1);
    expect(calls.toggleArchive).toHaveBeenCalledTimes(1);
  });

  it("flips the three labels with the record's state", () => {
    expect(labels({ ...OWNER, locked: true, restricted: true })).toEqual([
      PAGE_ACTION_OPEN_IN_NEW_TAB,
      PAGE_ACTION_COPY_LINK,
      PAGE_ACTION_DUPLICATE,
      PAGE_ACTION_UNLOCK,
      PAGE_ACTION_MAKE_PUBLIC,
      PAGE_ACTION_ARCHIVE,
    ]);
    expect(PAGE_ACTION_UNLOCK).toBe("Unlock");
    expect(PAGE_ACTION_MAKE_PUBLIC).toBe("Make public");
  });

  it("hides visibility on a shelved page and offers restore and delete instead", () => {
    expect(labels({ ...OWNER, archived: true })).toEqual([
      PAGE_ACTION_OPEN_IN_NEW_TAB,
      PAGE_ACTION_COPY_LINK,
      PAGE_ACTION_DUPLICATE,
      PAGE_ACTION_LOCK,
      PAGE_ACTION_RESTORE,
      PAGE_ACTION_DELETE,
    ]);
    expect(PAGE_ACTION_RESTORE).toBe("Restore");
    expect(PAGE_ACTION_DELETE).toBe("Delete");
  });

  it("offers deletion only for a shelved page", () => {
    expect(labels(OWNER)).not.toContain(PAGE_ACTION_DELETE);
    expect(labels({ ...OWNER, archived: true })).toContain(PAGE_ACTION_DELETE);
  });

  it("offers the cross-project move only where the edition enables it", () => {
    expect(labels(OWNER)).not.toContain(PAGE_ACTION_MOVE);
    expect(labels({ ...OWNER, move: true })).toContain(PAGE_ACTION_MOVE);
    expect(PAGE_ACTION_MOVE).toBe("Move to project…");
  });

  it("keeps a member's menu free of every state switch", () => {
    const entries = labels(MEMBER);
    for (const entry of [
      PAGE_ACTION_LOCK,
      PAGE_ACTION_UNLOCK,
      PAGE_ACTION_MAKE_PRIVATE,
      PAGE_ACTION_MAKE_PUBLIC,
      PAGE_ACTION_ARCHIVE,
      PAGE_ACTION_RESTORE,
      PAGE_ACTION_DELETE,
      PAGE_ACTION_MOVE,
    ]) {
      expect(entries).not.toContain(entry);
    }
  });

  it("pins the duplicate and state-switch notices", () => {
    expect(PAGE_DUPLICATED_TITLE).toBe("Page duplicated successfully.");
    expect(PAGE_DUPLICATE_FAILED_TITLE).toBe("Page could not be duplicated.");
    expect(PAGE_LOCKED_TITLE).toBe("Page locked successfully.");
    expect(PAGE_UNLOCKED_TITLE).toBe("Page unlocked successfully.");
    expect(PAGE_LOCK_FAILED_TITLE).toBe("Page could not be locked.");
    expect(PAGE_ARCHIVED_TITLE).toBe("Page archived successfully.");
    expect(PAGE_RESTORED_TITLE).toBe("Page restored successfully.");
    expect(PAGE_ARCHIVE_FAILED_TITLE).toBe("Page could not be archived.");
    expect(PAGE_RESTORE_FAILED_TITLE).toBe("Page could not be restored.");
    expect(PAGE_DELETED_TITLE).toBe("Page deleted successfully.");
    expect(PAGE_DELETE_FAILED_TITLE).toBe("Page could not be deleted.");
    expect(PAGE_MOVED_TITLE).toBe("Page moved to the new project.");
    expect(PAGE_MOVE_FAILED_TITLE).toBe("Page could not be moved.");
    expect(PAGE_VISIBILITY_FAILED_TITLE).toBe("Page visibility could not be changed.");
    expect(PAGE_VISIBILITY_SUFFIX).toBe("marked private");
    expect(PAGE_VISIBILITY_PUBLIC_SUFFIX).toBe("marked public");
  });
});
