// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row and card identity rules (parity/specs/modules.md §3.5, §3.6,
// MOD-012, MOD-020): which progress marker a row carries, how the work-item
// mix becomes bar segments, and whether the card shows a lead avatar or the
// no-lead marker.

import { describe, expect, it } from "vitest";

import { makeDirectoryEntry, MODULES_LEAD_ID, makeModuleRow } from "../testing.js";
import {
  MODULE_SEGMENT_COLORS,
  MODULE_SEGMENT_GROUPS,
  MODULE_SEGMENT_LABELS,
  moduleLeadSlot,
  modulePercentFloor,
  moduleProgressMarker,
  moduleProgressSegments,
  moduleWorkItemCountText,
  resolveModuleLead,
} from "./moduleIdentity.js";

describe("moduleProgressMarker", () => {
  it("shows the floored percentage while work remains", () => {
    expect(moduleProgressMarker(makeModuleRow({ total_issues: 4, completed_issues: 1 }))).toEqual({
      kind: "percent",
      percent: 25,
    });
  });

  it("counts cancelled work items as done in the percentage", () => {
    expect(moduleProgressMarker(makeModuleRow({ total_issues: 4, completed_issues: 1, cancelled_issues: 1 }))).toEqual({
      kind: "percent",
      percent: 50,
    });
  });

  it("floors a fractional share instead of rounding it up", () => {
    expect(moduleProgressMarker(makeModuleRow({ total_issues: 3, completed_issues: 2 }))).toEqual({
      kind: "percent",
      percent: 66,
    });
  });

  it("shows a check once every work item is done", () => {
    expect(moduleProgressMarker(makeModuleRow({ total_issues: 2, completed_issues: 2 }))).toEqual({ kind: "check" });
    expect(moduleProgressMarker(makeModuleRow({ total_issues: 2, completed_issues: 1, cancelled_issues: 1 }))).toEqual({
      kind: "check",
    });
  });

  it("shows a check even when the status lags behind completion", () => {
    expect(
      moduleProgressMarker(makeModuleRow({ status: "in-progress", total_issues: 1, completed_issues: 1 }))
    ).toEqual({ kind: "check" });
  });

  it("warns when the module is finished but its work items are not", () => {
    expect(moduleProgressMarker(makeModuleRow({ status: "completed", total_issues: 2, completed_issues: 1 }))).toEqual({
      kind: "alert",
    });
  });

  it("warns for a finished module with no work items at all", () => {
    expect(moduleProgressMarker(makeModuleRow({ status: "completed" }))).toEqual({ kind: "alert" });
  });

  it("keeps the percentage for a cancelled module, not the alert", () => {
    expect(moduleProgressMarker(makeModuleRow({ status: "cancelled", total_issues: 2, completed_issues: 1 }))).toEqual({
      kind: "percent",
      percent: 50,
    });
  });

  it("reports zero percent for a module without work items", () => {
    expect(moduleProgressMarker(makeModuleRow())).toEqual({ kind: "percent", percent: 0 });
  });
});

describe("modulePercentFloor", () => {
  it("is zero without work items", () => {
    expect(modulePercentFloor({ completed_issues: 3, cancelled_issues: 0, total_issues: 0 })).toBe(0);
  });

  it("never exceeds 100 when stale counts overrun the total", () => {
    expect(modulePercentFloor({ completed_issues: 5, cancelled_issues: 0, total_issues: 2 })).toBe(100);
  });

  it("never goes below zero", () => {
    expect(modulePercentFloor({ completed_issues: -2, cancelled_issues: 0, total_issues: 4 })).toBe(0);
  });
});

describe("moduleProgressSegments", () => {
  it("draws one slice per populated group in reading order", () => {
    const segments = moduleProgressSegments(
      makeModuleRow({
        total_issues: 4,
        completed_issues: 2,
        started_issues: 1,
        backlog_issues: 1,
      })
    );
    expect(segments.map((segment) => segment.group)).toEqual(["completed", "started", "backlog"]);
    expect(segments.map((segment) => segment.count)).toEqual([2, 1, 1]);
    expect(segments.map((segment) => segment.width)).toEqual([50, 25, 25]);
    expect(segments[0]?.color).toBe(MODULE_SEGMENT_COLORS.completed);
    expect(segments[0]?.label).toBe(MODULE_SEGMENT_LABELS.completed);
  });

  it("orders every group as the group list does", () => {
    const segments = moduleProgressSegments(
      makeModuleRow({
        total_issues: 5,
        cancelled_issues: 1,
        unstarted_issues: 1,
        completed_issues: 1,
        backlog_issues: 1,
        started_issues: 1,
      })
    );
    expect(segments.map((segment) => segment.group)).toEqual([...MODULE_SEGMENT_GROUPS]);
  });

  it("draws nothing for a module without work items", () => {
    expect(moduleProgressSegments(makeModuleRow())).toEqual([]);
  });

  it("omits a group that holds no work items", () => {
    const segments = moduleProgressSegments(makeModuleRow({ total_issues: 2, started_issues: 2 }));
    expect(segments).toHaveLength(1);
    expect(segments[0]).toMatchObject({ group: "started", width: 100 });
  });

  it("treats a malformed count as absent and a stale one as the whole bar", () => {
    const row = makeModuleRow({ total_issues: 2, completed_issues: 3, started_issues: Number.NaN });
    const segments = moduleProgressSegments(row);
    expect(segments).toHaveLength(1);
    expect(segments[0]).toMatchObject({ group: "completed", count: 3, width: 100 });
  });

  it("drops a negative count", () => {
    expect(moduleProgressSegments(makeModuleRow({ total_issues: 2, completed_issues: -1 }))).toEqual([]);
  });
});

describe("moduleLeadSlot", () => {
  it("names the lead when one is set", () => {
    expect(moduleLeadSlot(makeModuleRow({ lead_id: MODULES_LEAD_ID }))).toEqual({
      kind: "avatar",
      memberId: MODULES_LEAD_ID,
    });
  });

  it("falls back to the no-lead mark", () => {
    expect(moduleLeadSlot(makeModuleRow())).toEqual({ kind: "none" });
  });
});

describe("resolveModuleLead", () => {
  it("resolves the lead's name and picture from the directory", () => {
    const entry = makeDirectoryEntry({ member: { avatar_url: "https://cdn.test/ada.png" } });
    expect(resolveModuleLead(makeModuleRow({ lead_id: MODULES_LEAD_ID }), [entry])).toEqual({
      kind: "avatar",
      person: { name: "Ada Lovelace", avatarUrl: "https://cdn.test/ada.png" },
    });
  });

  it("shows the no-lead mark for a module without a lead", () => {
    expect(resolveModuleLead(makeModuleRow(), [makeDirectoryEntry()])).toEqual({ kind: "none" });
  });

  it("shows the no-lead mark when the lead left the readable directory", () => {
    expect(resolveModuleLead(makeModuleRow({ lead_id: MODULES_LEAD_ID }), [])).toEqual({ kind: "none" });
  });

  it("shows the no-lead mark before the directory arrives", () => {
    expect(resolveModuleLead(makeModuleRow({ lead_id: MODULES_LEAD_ID }), undefined)).toEqual({ kind: "none" });
  });
});

describe("moduleWorkItemCountText", () => {
  it("reads done over total", () => {
    expect(moduleWorkItemCountText(makeModuleRow({ total_issues: 2, completed_issues: 1 }))).toBe("1/2 work items");
  });

  it("keeps the noun singular for one work item", () => {
    expect(moduleWorkItemCountText(makeModuleRow({ total_issues: 1, completed_issues: 0 }))).toBe("0/1 work item");
  });

  it("says zero work items for an empty module", () => {
    expect(moduleWorkItemCountText(makeModuleRow())).toBe("0 work items");
  });
});
