// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Every module read nests under one workspace-scoped prefix, so a
// workspace switch or sign-out clears the subtree, and the prefixes later
// slices invalidate stay distinct from one another.

import { describe, expect, it } from "vitest";

import { moduleKeys } from "./keys.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

describe("moduleKeys", () => {
  it("scopes every read under the workspace", () => {
    expect(moduleKeys.all("acme")).toEqual(["ws", "acme", "modules"]);
    expect(moduleKeys.list("acme", PROJECT)).toEqual(["ws", "acme", "modules", "list", PROJECT]);
    expect(moduleKeys.detail("acme", PROJECT, MODULE)).toEqual(["ws", "acme", "modules", "detail", PROJECT, MODULE]);
    expect(moduleKeys.archivedList("acme", PROJECT)).toEqual(["ws", "acme", "modules", "archived", PROJECT]);
    expect(moduleKeys.archivedDetail("acme", PROJECT, MODULE)).toEqual([
      "ws",
      "acme",
      "modules",
      "archived",
      PROJECT,
      MODULE,
    ]);
    expect(moduleKeys.workspaceList("acme")).toEqual(["ws", "acme", "modules", "workspace"]);
    expect(moduleKeys.projectEstimates("acme", PROJECT)).toEqual(["ws", "acme", "modules", "estimates", PROJECT]);
  });

  it("nests the links and work-item reads beneath the module detail", () => {
    const detail = moduleKeys.detail("acme", PROJECT, MODULE);
    expect(moduleKeys.links("acme", PROJECT, MODULE)).toEqual([...detail, "links"]);
    expect(moduleKeys.workItems("acme", PROJECT, MODULE)).toEqual([...detail, "work-items"]);
  });

  it("keeps one project's reads out of another's", () => {
    const other = "223e4567-e89b-12d3-a456-426614174099";
    expect(moduleKeys.list("acme", PROJECT)).not.toEqual(moduleKeys.list("acme", other));
    expect(moduleKeys.list("acme", PROJECT)).not.toEqual(moduleKeys.list("other", PROJECT));
  });
});
