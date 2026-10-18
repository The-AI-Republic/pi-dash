// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { isCompletedModule, selectModuleOptions } from "./modules.js";

const ALPHA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BETA = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const PROJECT = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

function moduleRow(id: string, name: string) {
  return { id, project_id: PROJECT, name, status: "planned", is_favorite: false };
}

describe("selectModuleOptions", () => {
  it("sorts selected-first in multi mode with no clear row", () => {
    const options = selectModuleOptions({
      modules: [moduleRow(ALPHA, "Alpha"), moduleRow(BETA, "Beta")],
      selectedIds: [BETA],
      single: false,
    });
    expect(options).toEqual([
      { id: BETA, name: "Beta", selected: true, clear: false },
      { id: ALPHA, name: "Alpha", selected: false, clear: false },
    ]);
  });

  it("leads single mode with the clear row", () => {
    const options = selectModuleOptions({
      modules: [moduleRow(ALPHA, "Alpha")],
      selectedIds: [],
      single: true,
    });
    expect(options.map((option) => option.name)).toEqual(["No module", "Alpha"]);
  });

  it("hides the clear row on request", () => {
    const options = selectModuleOptions({
      modules: [moduleRow(ALPHA, "Alpha")],
      selectedIds: [],
      single: true,
      allowClear: false,
    });
    expect(options.map((option) => option.name)).toEqual(["Alpha"]);
  });
});

describe("isCompletedModule", () => {
  it("matches completed modules case-insensitively", () => {
    expect(isCompletedModule("completed")).toBe(true);
    expect(isCompletedModule("COMPLETED")).toBe(true);
  });

  it("keeps every other module status editable", () => {
    for (const status of ["backlog", "planned", "in-progress", "paused", "cancelled", undefined]) {
      expect(isCompletedModule(status)).toBe(false);
    }
  });
});
