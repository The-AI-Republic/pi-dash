// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Scope bases (NEWFRONT-540, RUN-002/003/004). Every tab, rail and detail
// link derives from these helpers, so the workspace and project trees can
// never drift apart.
import { describe, expect, it } from "vitest";

import { runnersBase, runnersPath } from "./scope.js";

describe("runnersBase", () => {
  it("builds the workspace aggregate base", () => {
    expect(runnersBase({ workspaceSlug: "acme" })).toBe("/acme/runners");
  });

  it("builds the project-scoped base when a project id is present", () => {
    expect(runnersBase({ workspaceSlug: "acme", projectId: "p-1" })).toBe("/acme/projects/p-1/runners");
  });
});

describe("runnersPath", () => {
  it("returns the base when no child is named", () => {
    expect(runnersPath({ workspaceSlug: "acme" })).toBe("/acme/runners");
  });

  it("appends a child path in either scope", () => {
    expect(runnersPath({ workspaceSlug: "acme" }, "runs")).toBe("/acme/runners/runs");
    expect(runnersPath({ workspaceSlug: "acme", projectId: "p-1" }, "chat/r-1")).toBe(
      "/acme/projects/p-1/runners/chat/r-1"
    );
  });
});
