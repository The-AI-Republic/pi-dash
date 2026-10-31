// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { INTAKE_STATUS } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  appliedFilterCount,
  applyCustomDate,
  clearFilterGroup,
  defaultSelectionForTab,
  filterActivity,
  INTAKE_FILTER_SECTION_TITLES,
  INTAKE_FILTER_SECTIONS,
  pickDatePreset,
  selectionToListFilters,
  toggleFilterId,
  toggleFilterStatus,
} from "./filters.js";

const TODAY = "2026-10-23";

describe("intake filter selection (INT-009/010)", () => {
  it("lists seven sections in fixed order with titles", () => {
    expect([...INTAKE_FILTER_SECTIONS]).toEqual([
      "status",
      "priority",
      "assignees",
      "createdBy",
      "labels",
      "createdAt",
      "updatedAt",
    ]);
    expect(INTAKE_FILTER_SECTION_TITLES).toEqual({
      status: "Status",
      priority: "Priority",
      assignees: "Assignees",
      createdBy: "Created by",
      labels: "Labels",
      createdAt: "Created date",
      updatedAt: "Updated date",
    });
  });

  it("starts a tab session on the tab's default statuses", () => {
    expect(defaultSelectionForTab("open").statuses).toEqual([INTAKE_STATUS.pending]);
    expect(defaultSelectionForTab("closed").statuses).toEqual([
      INTAKE_STATUS.accepted,
      INTAKE_STATUS.declined,
      INTAKE_STATUS.duplicate,
    ]);
    const open = defaultSelectionForTab("open");
    expect(open.priorities).toEqual([]);
    expect(open.createdAt).toBe(null);
    expect(open.updatedAt).toBe(null);
  });

  it("toggles statuses and refuses to empty the section", () => {
    const open = defaultSelectionForTab("open");
    const widened = toggleFilterStatus(open, INTAKE_STATUS.snoozed);
    expect(widened.statuses).toEqual([INTAKE_STATUS.pending, INTAKE_STATUS.snoozed]);
    expect(toggleFilterStatus(widened, INTAKE_STATUS.snoozed).statuses).toEqual([INTAKE_STATUS.pending]);
    expect(toggleFilterStatus(open, INTAKE_STATUS.pending)).toBe(open);
    const closed = defaultSelectionForTab("closed");
    expect(toggleFilterStatus(closed, INTAKE_STATUS.declined).statuses).toEqual([
      INTAKE_STATUS.accepted,
      INTAKE_STATUS.duplicate,
    ]);
  });

  it("toggles member, priority and label ids", () => {
    const open = defaultSelectionForTab("open");
    const picked = toggleFilterId(open, "priorities", "urgent");
    expect(picked.priorities).toEqual(["urgent"]);
    expect(toggleFilterId(picked, "priorities", "urgent").priorities).toEqual([]);
    expect(toggleFilterId(open, "assignees", "u-1").assignees).toEqual(["u-1"]);
    expect(toggleFilterId(open, "createdBy", "u-1").createdBy).toEqual(["u-1"]);
    expect(toggleFilterId(open, "labels", "l-1").labels).toEqual(["l-1"]);
  });

  it("replaces the date value on preset pick and clears on re-pick", () => {
    const open = defaultSelectionForTab("open");
    const picked = pickDatePreset(open, "createdAt", "today");
    expect(picked.createdAt).toEqual({ kind: "preset", preset: "today" });
    expect(pickDatePreset(picked, "createdAt", "today").createdAt).toBe(null);
    const replaced = pickDatePreset(picked, "createdAt", "last7");
    expect(replaced.createdAt).toEqual({ kind: "preset", preset: "last7" });
    expect(pickDatePreset(open, "updatedAt", "today").updatedAt).toEqual({ kind: "preset", preset: "today" });
  });

  it("applies an explicit range over any preset", () => {
    const open = defaultSelectionForTab("open");
    const ranged = applyCustomDate(open, "createdAt", { kind: "custom-range", from: "2026-10-15", to: "2026-10-20" });
    expect(ranged.createdAt).toEqual({ kind: "custom-range", from: "2026-10-15", to: "2026-10-20" });
    const preset = pickDatePreset(ranged, "createdAt", "today");
    expect(preset.createdAt).toEqual({ kind: "preset", preset: "today" });
  });

  it("clears every group but status", () => {
    const selection = {
      ...defaultSelectionForTab("open"),
      priorities: ["urgent"],
      assignees: ["u-1"],
      createdBy: ["u-2"],
      labels: ["l-1"],
      createdAt: { kind: "preset", preset: "today" },
      updatedAt: { kind: "preset", preset: "today" },
    } as const;
    expect(clearFilterGroup(selection, "priority").priorities).toEqual([]);
    expect(clearFilterGroup(selection, "assignees").assignees).toEqual([]);
    expect(clearFilterGroup(selection, "createdBy").createdBy).toEqual([]);
    expect(clearFilterGroup(selection, "labels").labels).toEqual([]);
    expect(clearFilterGroup(selection, "createdAt").createdAt).toBe(null);
    expect(clearFilterGroup(selection, "updatedAt").updatedAt).toBe(null);
  });

  it("counts every selected value including the tab default", () => {
    expect(appliedFilterCount(defaultSelectionForTab("open"))).toBe(1);
    expect(appliedFilterCount(defaultSelectionForTab("closed"))).toBe(3);
    const selection = {
      ...toggleFilterStatus(defaultSelectionForTab("open"), INTAKE_STATUS.snoozed),
      priorities: ["urgent", "high"],
      createdAt: { kind: "preset", preset: "today" },
    } as const;
    expect(appliedFilterCount(selection)).toBe(5);
  });

  it("reports no activity for an untouched tab", () => {
    expect(filterActivity(defaultSelectionForTab("open"), "open")).toEqual({
      userFilterCount: 0,
      statusNarrowed: false,
    });
    expect(filterActivity(defaultSelectionForTab("closed"), "closed").statusNarrowed).toBe(false);
  });

  it("reports user choices and status narrowing separately", () => {
    const open = defaultSelectionForTab("open");
    expect(filterActivity(toggleFilterId(open, "priorities", "urgent"), "open")).toEqual({
      userFilterCount: 1,
      statusNarrowed: false,
    });
    expect(filterActivity(toggleFilterStatus(open, INTAKE_STATUS.snoozed), "open")).toEqual({
      userFilterCount: 0,
      statusNarrowed: true,
    });
    const narrowed = toggleFilterStatus(defaultSelectionForTab("closed"), INTAKE_STATUS.accepted);
    expect(filterActivity(narrowed, "closed").statusNarrowed).toBe(true);
  });

  it("maps a selection to the list read with the tab's wire order", () => {
    const selection = toggleFilterStatus(defaultSelectionForTab("closed"), INTAKE_STATUS.declined);
    const filters = selectionToListFilters("closed", selection, TODAY);
    expect(filters.statuses).toEqual([INTAKE_STATUS.accepted, INTAKE_STATUS.duplicate]);
    expect(filters.priorities).toBe(undefined);
    expect(filters.createdAt).toBe(undefined);
  });

  it("carries multi-values and encoded dates, omitting empties", () => {
    const selection = {
      ...defaultSelectionForTab("open"),
      priorities: ["urgent", "high"],
      assignees: ["u-1"],
      createdAt: { kind: "preset", preset: "today" },
      updatedAt: { kind: "custom-range", from: "2026-10-15", to: "2026-10-20" },
    } as const;
    const filters = selectionToListFilters("open", selection, TODAY);
    expect(filters.priorities).toEqual(["urgent", "high"]);
    expect(filters.assignees).toEqual(["u-1"]);
    expect(filters.createdBy).toBe(undefined);
    expect(filters.labels).toBe(undefined);
    expect(filters.createdAt).toBe(`${TODAY};after,${TODAY};before`);
    expect(filters.updatedAt).toBe("2026-10-15;after,2026-10-20;before");
  });

  it("carries creator and label values", () => {
    const selection = {
      ...defaultSelectionForTab("open"),
      createdBy: ["u-2"],
      labels: ["l-1", "l-2"],
    };
    const filters = selectionToListFilters("open", selection, TODAY);
    expect(filters.createdBy).toEqual(["u-2"]);
    expect(filters.labels).toEqual(["l-1", "l-2"]);
  });

  it("defaults the token day to the UTC today", () => {
    const selection = { ...defaultSelectionForTab("open"), createdAt: { kind: "preset", preset: "today" } } as const;
    const today = new Date().toISOString().slice(0, 10);
    expect(selectionToListFilters("open", selection).createdAt).toBe(`${today};after,${today};before`);
  });
});
