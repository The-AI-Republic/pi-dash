// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Project } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { filterOverflowProjects, hasOverflow } from "./overflow.js";

function row(name: string, identifier: string): Project {
  return { name, identifier } as Project;
}

const ROWS = [row("Parity Sidebar Overflow", "PAR_SO"), row("Parity Sidebar Tertiary", "PAR_ST")];

describe("hasOverflow", () => {
  it("shows the toggle once the cap leaves rows out", () => {
    expect(hasOverflow(2, 1)).toBe(true);
  });

  it("hides the toggle when everything fits or no cap applies", () => {
    expect(hasOverflow(2, 2)).toBe(false);
    expect(hasOverflow(1, 2)).toBe(false);
    expect(hasOverflow(2, null)).toBe(false);
    expect(hasOverflow(0, 1)).toBe(false);
  });
});

describe("filterOverflowProjects", () => {
  it("keeps every row on a blank query", () => {
    expect(filterOverflowProjects(ROWS, "")).toHaveLength(2);
    expect(filterOverflowProjects(ROWS, "   ")).toHaveLength(2);
  });

  it("matches names without case", () => {
    expect(filterOverflowProjects(ROWS, "Overflow").map((row) => row.name)).toEqual(["Parity Sidebar Overflow"]);
    expect(filterOverflowProjects(ROWS, "overflow").map((row) => row.name)).toEqual(["Parity Sidebar Overflow"]);
  });

  it("matches short codes as typed", () => {
    expect(filterOverflowProjects(ROWS, "_SO").map((row) => row.name)).toEqual(["Parity Sidebar Overflow"]);
    expect(filterOverflowProjects(ROWS, "_so")).toEqual([]);
  });

  it("matches nothing on a hopeless query", () => {
    expect(filterOverflowProjects(ROWS, "zzz-no-such-project")).toEqual([]);
  });
});
