// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row-menu entries per role and cycle state (parity/specs/cycles.md
// §10, CYC-028): the fixed titles in spec order, the archive gate
// with its hint, and the finished/archived predicates behind the
// read-only gating (CYC-023).

import { describe, expect, it } from "vitest";

import { makeCycleRow } from "../testing.js";
import { CYCLE_ARCHIVE_GATE_HINT, cycleMenuEntries, isCycleArchived, isCycleFinished } from "./cycleMenu.js";

function titlesOf(entries: ReturnType<typeof cycleMenuEntries>): string[] {
  return entries.map((entry) => entry.title);
}

describe("isCycleFinished", () => {
  it("reads the completed marker case-insensitively", () => {
    expect(isCycleFinished(makeCycleRow({ status: "COMPLETED" }))).toBe(true);
    expect(isCycleFinished(makeCycleRow({ status: "completed" }))).toBe(true);
    expect(isCycleFinished(makeCycleRow({ status: "Completed" }))).toBe(true);
  });

  it("rejects every other marker and a missing one", () => {
    for (const status of ["current", "upcoming", "draft", undefined]) {
      expect(isCycleFinished(makeCycleRow({ status }))).toBe(false);
    }
  });
});

describe("isCycleArchived", () => {
  it("reads the archive stamp, not the lifecycle marker", () => {
    expect(isCycleArchived(makeCycleRow({ archived_at: "2026-03-01T00:00:00Z" }))).toBe(true);
    expect(isCycleArchived(makeCycleRow({ archived_at: null }))).toBe(false);
    expect(isCycleArchived(makeCycleRow({}))).toBe(false);
  });
});

describe("cycleMenuEntries", () => {
  it("offers the full matrix on a draft row, archive gated with its hint", () => {
    const entries = cycleMenuEntries(makeCycleRow({ status: "draft" }), true);
    expect(titlesOf(entries)).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
    const archive = entries.find((entry) => entry.title === "Archive");
    expect(archive?.disabled).toBe(true);
    expect(archive?.hint).toBe(CYCLE_ARCHIVE_GATE_HINT);
    for (const entry of entries) {
      if (entry.title !== "Archive") expect(entry.disabled).toBe(false);
    }
  });

  it("gates archive the same way on current and upcoming rows", () => {
    for (const status of ["current", "upcoming"]) {
      const entries = cycleMenuEntries(makeCycleRow({ status }), true);
      expect(titlesOf(entries)).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
      const archive = entries.find((entry) => entry.title === "Archive");
      expect(archive?.disabled).toBe(true);
      expect(archive?.hint).toMatch(/completed/i);
    }
  });

  it("drops edit and delete on a finished row and enables archive with no hint", () => {
    const entries = cycleMenuEntries(makeCycleRow({ status: "COMPLETED" }), true);
    expect(titlesOf(entries)).toEqual(["Open in new tab", "Copy link", "Archive"]);
    const archive = entries.find((entry) => entry.title === "Archive");
    expect(archive?.disabled).toBe(false);
    expect(archive?.hint).toBe(null);
  });

  it("swaps archive for restore on an archived row", () => {
    const entries = cycleMenuEntries(makeCycleRow({ status: "COMPLETED", archived_at: "2026-03-01T00:00:00Z" }), true);
    expect(titlesOf(entries)).toEqual(["Open in new tab", "Copy link", "Restore"]);
    expect(entries.every((entry) => !entry.disabled)).toBe(true);
  });

  it("shows guests only the link actions whatever the state", () => {
    const rows = [
      makeCycleRow({ status: "draft" }),
      makeCycleRow({ status: "COMPLETED" }),
      makeCycleRow({ status: "COMPLETED", archived_at: "2026-03-01T00:00:00Z" }),
    ];
    for (const row of rows) {
      const entries = cycleMenuEntries(row, false);
      expect(titlesOf(entries)).toEqual(["Open in new tab", "Copy link"]);
      expect(entries.every((entry) => !entry.disabled)).toBe(true);
    }
  });
});
