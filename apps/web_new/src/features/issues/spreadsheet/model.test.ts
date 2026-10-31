// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  activeSheetSort,
  isSheetColumnKey,
  isSheetRowReadOnly,
  moveSheetFocus,
  parseStoredSheetSort,
  readStoredSheetSort,
  resolveSheetSort,
  serializeSheetSort,
  SHEET_COLUMNS,
  sheetCellId,
  sheetFirstCellId,
  sheetSortOrder,
  sheetSortState,
  SHEET_SORT_STORAGE_KEY,
  visibleSheetColumns,
  writeStoredSheetSort,
} from "./model.js";
import { isEditableColumn } from "./cells.js";

const shown = {};

describe("spreadsheet model", () => {
  it("keeps the spec column order and knows every key", () => {
    expect(SHEET_COLUMNS.map((column) => column.key)).toEqual([
      "state",
      "priority",
      "assignee",
      "labels",
      "modules",
      "cycle",
      "start_date",
      "due_date",
      "estimate",
      "created_on",
      "updated_on",
      "link",
      "attachment_count",
      "sub_issue_count",
    ]);
    for (const column of SHEET_COLUMNS) expect(isSheetColumnKey(column.key)).toBe(true);
    expect(isSheetColumnKey("nope")).toBe(false);
  });

  it("shows enabled properties and hides disabled ones", () => {
    const columns = visibleSheetColumns({ state: false, labels: false }, { hideCycle: false, hideModule: false });
    const keys = columns.map((column) => column.key);
    expect(keys).not.toContain("state");
    expect(keys).not.toContain("labels");
    expect(keys).toContain("priority");
    expect(visibleSheetColumns(shown, { hideCycle: false, hideModule: false })).toHaveLength(SHEET_COLUMNS.length);
  });

  it("drops cycle and module while their project features are off", () => {
    const keys = visibleSheetColumns(shown, { hideCycle: true, hideModule: true }).map((column) => column.key);
    expect(keys).not.toContain("cycle");
    expect(keys).not.toContain("modules");
    expect(keys).toContain("state");
  });

  it("drops the estimate column while the project has no estimate system", () => {
    const keys = visibleSheetColumns(shown, { hideCycle: false, hideModule: false, hideEstimate: true }).map(
      (column) => column.key
    );
    expect(keys).not.toContain("estimate");
    expect(visibleSheetColumns(shown, { hideCycle: false, hideModule: false }).map((column) => column.key)).toContain(
      "estimate"
    );
  });

  it("reads a column's sort state from order_by", () => {
    const state = SHEET_COLUMNS[0]!;
    expect(sheetSortState(state, "state__name")).toEqual({
      column: "state",
      direction: "ascending",
      orderBy: "state__name",
    });
    expect(sheetSortState(state, "-state__name")).toMatchObject({ direction: "descending" });
    expect(sheetSortState(state, "-created_at")).toBeNull();
    expect(sheetSortState({ key: "x", label: "X", sort: { ascending: "x", descending: "-x" } }, "y")).toBeNull();
  });

  it("finds the one active sort across all columns", () => {
    expect(activeSheetSort(SHEET_COLUMNS, "-created_at")).toBeNull();
    expect(activeSheetSort(SHEET_COLUMNS, "state__name")?.column).toBe("state");
    expect(activeSheetSort(SHEET_COLUMNS, "target_date")?.column).toBe("due_date");
    expect(activeSheetSort(SHEET_COLUMNS, "not_a_sort_key")).toBeNull();
  });

  it("uses the remembered pair to distinguish created-on ascending from the default", () => {
    const remembered = { column: "created_on", direction: "ascending" as const, orderBy: "-created_at" };
    expect(resolveSheetSort(SHEET_COLUMNS, "-created_at", null)).toBeNull();
    expect(resolveSheetSort(SHEET_COLUMNS, "-created_at", remembered)).toEqual(remembered);
    expect(
      resolveSheetSort(SHEET_COLUMNS, "state__name", {
        column: "created_on",
        direction: "ascending",
        orderBy: "-created_at",
      })
    ).toMatchObject({ column: "state", direction: "ascending" });
    expect(
      resolveSheetSort(SHEET_COLUMNS, "-created_at", { column: "nope", direction: "ascending", orderBy: "-created_at" })
    ).toBeNull();
    expect(
      resolveSheetSort(SHEET_COLUMNS, "state__name", {
        column: "state",
        direction: "descending",
        orderBy: "state__name",
      })
    ).toMatchObject({ column: "state", direction: "ascending" });
  });

  it("writes the ascending or descending key", () => {
    const priority = SHEET_COLUMNS.find((column) => column.key === "priority")!;
    expect(sheetSortOrder(priority, "ascending")).toBe("priority");
    expect(sheetSortOrder(priority, "descending")).toBe("-priority");
  });

  it("only marks the spec's editable columns", () => {
    for (const key of [
      "state",
      "priority",
      "assignee",
      "start_date",
      "due_date",
      "estimate",
      "labels",
      "cycle",
      "modules",
    ]) {
      expect(isEditableColumn(key)).toBe(true);
    }
    for (const key of ["created_on", "updated_on", "link", "attachment_count", "sub_issue_count"]) {
      expect(isEditableColumn(key)).toBe(false);
    }
  });

  it("locks archived and completed-cycle rows but not live rows", () => {
    const cycles = [
      { id: "done", status: "completed" },
      { id: "live", status: "active" },
    ];
    expect(isSheetRowReadOnly({ archived_at: "2026-01-01", cycle_id: null }, cycles)).toBe(true);
    expect(isSheetRowReadOnly({ archived_at: null, cycle_id: "done" }, cycles)).toBe(true);
    expect(isSheetRowReadOnly({ archived_at: null, cycle_id: "live" }, cycles)).toBe(false);
    expect(isSheetRowReadOnly({ archived_at: null, cycle_id: "missing" }, cycles)).toBe(false);
    expect(isSheetRowReadOnly({ archived_at: null, cycle_id: null }, cycles)).toBe(false);
  });

  it("round-trips the local sort pair and rejects bad records", () => {
    const state = { column: "state", direction: "ascending" as const, orderBy: "state__name" };
    expect(parseStoredSheetSort(serializeSheetSort(state))).toEqual(state);
    expect(serializeSheetSort(null)).toBe("");
    expect(parseStoredSheetSort(null)).toBeNull();
    expect(parseStoredSheetSort("")).toBeNull();
    expect(parseStoredSheetSort("{")).toBeNull();
    expect(parseStoredSheetSort("[]")).toBeNull();
    expect(parseStoredSheetSort("5")).toBeNull();
    expect(parseStoredSheetSort(JSON.stringify({ column: "state", direction: "up", orderBy: "x" }))).toBeNull();
    expect(parseStoredSheetSort(JSON.stringify({ column: 1, direction: "ascending", orderBy: "x" }))).toBeNull();
  });

  describe("persisted sort storage", () => {
    afterEach(() => {
      vi.unstubAllGlobals();
    });

    function store(entries: Map<string, string>): Storage {
      return {
        length: entries.size,
        clear: () => entries.clear(),
        getItem: (key: string) => entries.get(key) ?? null,
        key: (index: number) => [...entries.keys()][index] ?? null,
        removeItem: (key: string) => {
          entries.delete(key);
        },
        setItem: (key: string, value: string) => {
          entries.set(key, value);
        },
      };
    }

    it("round-trips the remembered sort through the browser store", () => {
      const entries = new Map<string, string>();
      vi.stubGlobal("localStorage", store(entries));
      const state = { column: "state", direction: "descending" as const, orderBy: "-state__name" };

      writeStoredSheetSort(state);
      expect(entries.get(SHEET_SORT_STORAGE_KEY)).toBe(serializeSheetSort(state));
      expect(readStoredSheetSort()).toEqual(state);

      writeStoredSheetSort(null);
      expect(entries.get(SHEET_SORT_STORAGE_KEY)).toBe("");
      expect(readStoredSheetSort()).toBeNull();
    });

    it("reads null and drops writes when the host has no storage", () => {
      vi.stubGlobal("localStorage", undefined);
      const state = { column: "state", direction: "ascending" as const, orderBy: "state__name" };

      expect(readStoredSheetSort()).toBeNull();
      expect(() => writeStoredSheetSort(state)).not.toThrow();
    });

    it("reads null and drops writes when the storage getter itself throws", () => {
      const descriptor = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
      Object.defineProperty(globalThis, "localStorage", {
        configurable: true,
        get() {
          throw new Error("localStorage needs --localstorage-file");
        },
      });
      try {
        expect(readStoredSheetSort()).toBeNull();
        expect(() => writeStoredSheetSort(null)).not.toThrow();
      } finally {
        if (descriptor) Object.defineProperty(globalThis, "localStorage", descriptor);
        else delete (globalThis as { localStorage?: unknown }).localStorage;
      }
    });

    it("survives a store that refuses reads and writes", () => {
      vi.stubGlobal("localStorage", {
        ...store(new Map()),
        getItem: () => {
          throw new Error("denied");
        },
        setItem: () => {
          throw new Error("quota");
        },
      });
      expect(readStoredSheetSort()).toBeNull();
      expect(() => writeStoredSheetSort(null)).not.toThrow();
    });
  });

  it("clamps arrow movement at the grid edges", () => {
    expect(moveSheetFocus({ row: 1, column: 1 }, "up", 3, 3)).toEqual({ row: 0, column: 1 });
    expect(moveSheetFocus({ row: 1, column: 1 }, "down", 3, 3)).toEqual({ row: 2, column: 1 });
    expect(moveSheetFocus({ row: 1, column: 1 }, "left", 3, 3)).toEqual({ row: 1, column: 0 });
    expect(moveSheetFocus({ row: 1, column: 1 }, "right", 3, 3)).toEqual({ row: 1, column: 2 });
    expect(moveSheetFocus({ row: 0, column: 0 }, "up", 0, 0)).toEqual({ row: 0, column: 0 });
    expect(moveSheetFocus({ row: 99, column: 99 }, "down", 2, 2)).toEqual({ row: 1, column: 1 });
  });

  it("builds stable cell and row ids", () => {
    expect(sheetCellId("abc", "state")).toBe("sheet-cell-abc-state");
    expect(sheetFirstCellId("abc")).toBe("issue-abc");
  });
});
