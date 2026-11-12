// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quick-action model (parity/specs/issues.md §7, ISS-060–067): order,
// standing, archive gating and the list ellipsis.
import { describe, expect, it } from "vitest";

import {
  ARCHIVE_GATING_NOTE,
  canArchiveStateGroup,
  listMenuLabels,
  quickActionEntries,
  rowMenuLabel,
} from "./model.js";

const PROJECT_MENU = ["Edit", "Make a copy", "Open in new tab", "Copy link", "Move to project", "Archive", "Delete"];

function labels(
  context: Parameters<typeof quickActionEntries>[0]["context"],
  extra: Partial<{ canWrite: boolean; archivable: boolean; archived: boolean }> = {}
): string[] {
  return quickActionEntries({
    context,
    canWrite: extra.canWrite ?? true,
    archivable: extra.archivable ?? false,
    archived: extra.archived ?? false,
  }).map((entry) => entry.label);
}

describe("quickActionEntries", () => {
  it("lists the project menu in the order the oracle reads", () => {
    expect(labels("project")).toEqual(PROJECT_MENU);
    expect(labels("global")).toEqual(PROJECT_MENU);
  });

  it("offers only the read entries without write standing", () => {
    for (const context of ["project", "global", "cycle", "module", "archived"] as const) {
      expect(labels(context, { canWrite: false })).toEqual(["Open in new tab", "Copy link"]);
    }
    // The header contexts carry no copy-link entry: their headers already
    // own a copy control (ISS-066).
    expect(labels("detail", { canWrite: false })).toEqual(["Open in new tab"]);
  });

  it("disables archive with the note outside completed and cancelled states", () => {
    const [archive] = quickActionEntries({
      context: "project",
      canWrite: true,
      archivable: false,
      archived: false,
    }).filter((entry) => entry.key === "archive");
    expect(archive?.disabled).toBe(true);
    expect(archive?.description).toBe(ARCHIVE_GATING_NOTE);
    expect(labels("project")).toContain("Archive");
  });

  it("leaves archive enabled on a finished record", () => {
    const [archive] = quickActionEntries({
      context: "project",
      canWrite: true,
      archivable: true,
      archived: false,
    }).filter((entry) => entry.key === "archive");
    expect(archive?.disabled).toBe(false);
    expect(archive?.description).toBeUndefined();
  });

  it("swaps a stamped record from archive to restore in header contexts", () => {
    expect(labels("detail", { archived: true })).toEqual([
      "Make a copy",
      "Open in new tab",
      "Move to project",
      "Restore",
      "Delete",
    ]);
    expect(labels("detail")).toEqual(["Make a copy", "Open in new tab", "Move to project", "Archive", "Delete"]);
  });

  it("gives a cycle row its remove entry before archive", () => {
    expect(labels("cycle")).toEqual([
      "Edit",
      "Make a copy",
      "Open in new tab",
      "Copy link",
      "Remove from cycle",
      "Archive",
      "Delete",
    ]);
  });

  it("gives a module row its remove entry before archive", () => {
    expect(labels("module")).toEqual([
      "Edit",
      "Make a copy",
      "Open in new tab",
      "Copy link",
      "Remove from module",
      "Archive",
      "Delete",
    ]);
  });

  it("keeps archived rows to restore, open, copy and delete", () => {
    expect(labels("archived", { archived: true, canWrite: true })).toEqual([
      "Restore",
      "Open in new tab",
      "Copy link",
      "Delete",
    ]);
    // The same context without the stamp still lists the same set; the
    // archived list only ever holds stamped rows.
    expect(labels("archived")).toEqual(["Restore", "Open in new tab", "Copy link", "Delete"]);
  });
});

describe("canArchiveStateGroup", () => {
  it("admits completed and cancelled only", () => {
    expect(canArchiveStateGroup("completed")).toBe(true);
    expect(canArchiveStateGroup("cancelled")).toBe(true);
    expect(canArchiveStateGroup("started")).toBe(false);
    expect(canArchiveStateGroup(null)).toBe(false);
    expect(canArchiveStateGroup(undefined)).toBe(false);
  });
});

describe("rowMenuLabel", () => {
  it("names the record", () => {
    expect(rowMenuLabel("WEB-12")).toBe("Actions for WEB-12");
  });
});

describe("listMenuLabels", () => {
  it("puts open-in-new-tab before copy-link, then edition extras", () => {
    expect(listMenuLabels()).toEqual([
      { key: "openInNewTab", label: "Open in new tab" },
      { key: "copyLink", label: "Copy link" },
    ]);
    expect(listMenuLabels([{ key: "export", label: "Export" }]).map((entry) => entry.label)).toEqual([
      "Open in new tab",
      "Copy link",
      "Export",
    ]);
  });
});
