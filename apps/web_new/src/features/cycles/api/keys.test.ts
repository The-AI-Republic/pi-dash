// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle keys nest under the workspace scope; the detail tuple adopts the
// NEWFRONT-859 read so both share one cache entry (CYC-045).

import { describe, expect, it } from "vitest";

import { cycleDetailKeys } from "./cycleDetail.js";
import { cycleKeys } from "./keys.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "323e4567-e89b-12d3-a456-426614174002";

describe("cycleKeys", () => {
  it("scopes every read under the workspace", () => {
    expect(cycleKeys.all("acme")).toEqual(["ws", "acme", "cycles"]);
    expect(cycleKeys.list("acme", PROJECT)).toEqual(["ws", "acme", "cycles", "list", PROJECT]);
    expect(cycleKeys.hero("acme", PROJECT)).toEqual(["ws", "acme", "cycles", "hero", PROJECT]);
    expect(cycleKeys.archived("acme", PROJECT)).toEqual(["ws", "acme", "cycles", "archived", PROJECT]);
    expect(cycleKeys.archivedDetail("acme", PROJECT, CYCLE)).toEqual([
      "ws",
      "acme",
      "cycles",
      "archived",
      PROJECT,
      CYCLE,
    ]);
    expect(cycleKeys.progress("acme", PROJECT, CYCLE)).toEqual(["ws", "acme", "cycles", "progress", PROJECT, CYCLE]);
    expect(cycleKeys.detailedProgress("acme", PROJECT, CYCLE)).toEqual([
      "ws",
      "acme",
      "cycles",
      "detailed-progress",
      PROJECT,
      CYCLE,
    ]);
    expect(cycleKeys.analytics("acme", PROJECT, CYCLE, "issues")).toEqual([
      "ws",
      "acme",
      "cycles",
      "analytics",
      PROJECT,
      CYCLE,
      "issues",
    ]);
    expect(cycleKeys.analytics("acme", PROJECT, CYCLE, "points")).toEqual([
      "ws",
      "acme",
      "cycles",
      "analytics",
      PROJECT,
      CYCLE,
      "points",
    ]);
    expect(cycleKeys.workspaceCycles("acme")).toEqual(["ws", "acme", "cycles", "workspace-cycles"]);
    expect(cycleKeys.workspaceActives("acme", null)).toEqual(["ws", "acme", "cycles", "workspace-actives", null]);
    expect(cycleKeys.workspaceActives("acme", "cursor-9")).toEqual([
      "ws",
      "acme",
      "cycles",
      "workspace-actives",
      "cursor-9",
    ]);
  });

  it("shares the detail cache with the NEWFRONT-859 read", () => {
    expect(cycleKeys.detail("acme", PROJECT, CYCLE)).toEqual(["ws", "acme", "cycles", "detail", PROJECT, CYCLE]);
    expect(cycleKeys.detail("acme", PROJECT, CYCLE)).toEqual(cycleDetailKeys.detail("acme", PROJECT, CYCLE));
  });
});
