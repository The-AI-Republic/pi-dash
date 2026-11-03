// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { layoutOptionList } from "./layoutOptions.js";

describe("layoutOptionList", () => {
  it("lists all five layouts in switcher order", () => {
    expect(layoutOptionList()).toEqual([
      { kind: "list", label: "List" },
      { kind: "kanban", label: "Board" },
      { kind: "calendar", label: "Calendar" },
      { kind: "spreadsheet", label: "Spreadsheet" },
      { kind: "gantt_chart", label: "Timeline" },
    ]);
    expect(layoutOptionList([])).toHaveLength(5);
  });

  it("hides the excluded kinds", () => {
    expect(layoutOptionList(["kanban", "gantt_chart"])).toEqual([
      { kind: "list", label: "List" },
      { kind: "calendar", label: "Calendar" },
      { kind: "spreadsheet", label: "Spreadsheet" },
    ]);
    expect(layoutOptionList(["list", "kanban", "calendar", "spreadsheet", "gantt_chart"])).toEqual([]);
  });
});
