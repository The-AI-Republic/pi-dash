// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Menu assembly per role and archive state: the entry order, the
// archive gate with its explanation, and the restore-only archived set.

import type { ModuleStatus } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { MODULE_ARCHIVE_GATE_HINT, moduleCanArchive, moduleMenuEntries } from "./moduleMenuModel.js";

const STATUSES: ModuleStatus[] = ["backlog", "planned", "in-progress", "paused", "completed", "cancelled"];

describe("module menu assembly", () => {
  it("offers edit, the link actions, archive and delete to editors on active modules", () => {
    const entries = moduleMenuEntries({ status: "planned", canEdit: true, archived: false });
    expect(entries.map((entry) => entry.title)).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
  });

  it.each(STATUSES)("gates archive on a finished status (%s)", (status) => {
    const entries = moduleMenuEntries({ status, canEdit: true, archived: false });
    const archive = entries.find((entry) => entry.id === "archive");
    expect(archive?.disabled).toBe(!moduleCanArchive(status));
  });

  it("explains the archive gate with the pinned phrase while disabled", () => {
    const entries = moduleMenuEntries({ status: "in-progress", canEdit: true, archived: false });
    const archive = entries.find((entry) => entry.id === "archive");
    expect(archive?.hint).toBe(MODULE_ARCHIVE_GATE_HINT);
    expect(archive?.hint ?? "").toContain("completed or cancelled");
  });

  it("drops the archive explanation once the status finishes", () => {
    for (const status of ["completed", "cancelled"] as const) {
      const entries = moduleMenuEntries({ status, canEdit: true, archived: false });
      const archive = entries.find((entry) => entry.id === "archive");
      expect(archive?.disabled).toBe(false);
      expect(archive?.hint).toBe(null);
    }
  });

  it("offers the link actions and restore to editors on archived modules", () => {
    const entries = moduleMenuEntries({ status: "completed", canEdit: true, archived: true });
    expect(entries.map((entry) => entry.title)).toEqual(["Open in new tab", "Copy link", "Restore"]);
    expect(entries.find((entry) => entry.id === "restore")?.disabled).toBe(false);
  });

  it("offers only the link actions to guests in either state", () => {
    for (const archived of [false, true]) {
      const entries = moduleMenuEntries({ status: "planned", canEdit: false, archived });
      expect(entries.map((entry) => entry.title)).toEqual(["Open in new tab", "Copy link"]);
    }
  });

  it("keeps every other entry enabled and unexplained", () => {
    const entries = moduleMenuEntries({ status: "planned", canEdit: true, archived: false });
    for (const entry of entries.filter((candidate) => candidate.id !== "archive")) {
      expect(entry.disabled).toBe(false);
      expect(entry.hint).toBe(null);
    }
  });
});
