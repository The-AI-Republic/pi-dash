// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";
import {
  projectIssuesPath,
  projectSettingsPath,
  selectCardMenuEntries,
  type CardMenuCallbacks,
  type CardMenuStanding,
} from "./cardMenu.js";

const CALLBACKS: CardMenuCallbacks = {
  onSettings: vi.fn(),
  onJoin: vi.fn(),
  onOpenInNewTab: vi.fn(),
  onCopyLink: vi.fn(),
  onRestore: vi.fn(),
  onDelete: vi.fn(),
  onArchive: vi.fn(),
};

function standing(overrides: Partial<CardMenuStanding> = {}): CardMenuStanding {
  return {
    archived: false,
    isMember: true,
    isAuthorized: true,
    isAdmin: true,
    ...overrides,
  };
}

function labels(entries: ReturnType<typeof selectCardMenuEntries>): string[] {
  return entries.map((entry) => ("label" in entry ? entry.label : "separator"));
}

describe("selectCardMenuEntries", () => {
  it("offers settings, copy-link and archive to project admins, never join", () => {
    expect(labels(selectCardMenuEntries(standing(), CALLBACKS))).toEqual(["Settings", "Copy link", "Archive"]);
  });

  it("withholds archive from members without the admin standing", () => {
    expect(labels(selectCardMenuEntries(standing({ isAdmin: false }), CALLBACKS))).toEqual(["Settings", "Copy link"]);
  });

  it("hides archive until its dialog is wired", () => {
    expect(labels(selectCardMenuEntries(standing(), { ...CALLBACKS, onArchive: null }))).toEqual([
      "Settings",
      "Copy link",
    ]);
  });

  it("offers join, open-in-tab and copy-link to outsiders", () => {
    const entries = selectCardMenuEntries(
      standing({ isMember: false, isAuthorized: false, isAdmin: false }),
      CALLBACKS
    );
    expect(labels(entries)).toEqual(["Join", "Open in new tab", "Copy link"]);
  });

  it("offers copy-link alone to guest members", () => {
    const entries = selectCardMenuEntries(standing({ isAuthorized: false, isAdmin: false }), CALLBACKS);
    expect(labels(entries)).toEqual(["Copy link"]);
  });

  it("offers restore and delete to project admins on archived cards", () => {
    const entries = selectCardMenuEntries(standing({ archived: true }), CALLBACKS);
    expect(labels(entries)).toEqual(["Restore", "Delete"]);
  });

  it("hides restore and delete until their dialogs are wired", () => {
    expect(labels(selectCardMenuEntries(standing({ archived: true }), { ...CALLBACKS, onRestore: null }))).toEqual([
      "Delete",
    ]);
    expect(labels(selectCardMenuEntries(standing({ archived: true }), { ...CALLBACKS, onDelete: null }))).toEqual([
      "Restore",
    ]);
    expect(
      labels(selectCardMenuEntries(standing({ archived: true }), { ...CALLBACKS, onRestore: null, onDelete: null }))
    ).toEqual([]);
  });

  it("offers nothing on archived cards to non-admins", () => {
    expect(labels(selectCardMenuEntries(standing({ archived: true, isAdmin: false }), CALLBACKS))).toEqual([]);
    expect(
      labels(
        selectCardMenuEntries(
          standing({ archived: true, isMember: false, isAuthorized: false, isAdmin: false }),
          CALLBACKS
        )
      )
    ).toEqual([]);
  });

  it("dispatches each entry to its callback", () => {
    const callbacks: CardMenuCallbacks = {
      onSettings: vi.fn(),
      onJoin: vi.fn(),
      onOpenInNewTab: vi.fn(),
      onCopyLink: vi.fn(),
      onRestore: vi.fn(),
      onDelete: vi.fn(),
      onArchive: vi.fn(),
    };
    for (const entry of selectCardMenuEntries(standing(), callbacks)) {
      if ("onSelect" in entry) entry.onSelect();
    }
    expect(callbacks.onSettings).toHaveBeenCalledTimes(1);
    expect(callbacks.onCopyLink).toHaveBeenCalledTimes(1);
    expect(callbacks.onArchive).toHaveBeenCalledTimes(1);
    for (const entry of selectCardMenuEntries(
      standing({ isMember: false, isAuthorized: false, isAdmin: false }),
      callbacks
    )) {
      if ("onSelect" in entry) entry.onSelect();
    }
    expect(callbacks.onJoin).toHaveBeenCalledTimes(1);
    expect(callbacks.onOpenInNewTab).toHaveBeenCalledTimes(1);
    for (const entry of selectCardMenuEntries(standing({ archived: true }), callbacks)) {
      if ("onSelect" in entry) entry.onSelect();
    }
    expect(callbacks.onRestore).toHaveBeenCalledTimes(1);
    expect(callbacks.onDelete).toHaveBeenCalledTimes(1);
  });
});

describe("card paths", () => {
  it("builds the work-items and settings addresses", () => {
    expect(projectIssuesPath("acme", "p1")).toBe("/acme/projects/p1/issues");
    expect(projectSettingsPath("acme", "p1")).toBe("/acme/settings/projects/p1");
  });
});
