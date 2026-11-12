// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: workspace search against live Django.
import { describe, expect, it } from "vitest";
import { searchWorkspace } from "../contracts/search.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

describe.skipIf(!contractEnabled)("search contract", () => {
  it("finds the seeded issue across every entity", async () => {
    const client = await sharedSignedIn();
    const results = await searchWorkspace(client, CONTRACT_WORKSPACE, {
      search: "Contract issue",
      workspaceSearch: true,
    });
    const issues = results.results.issue ?? [];
    expect(issues.length).toBeGreaterThanOrEqual(1);
    expect(issues[0]?.project__identifier).toBe("CT");
    expect(issues[0]?.workspace__slug).toBe(CONTRACT_WORKSPACE);
  });

  it("scopes to the requested entities only", async () => {
    const client = await sharedSignedIn();
    const results = await searchWorkspace(client, CONTRACT_WORKSPACE, {
      search: "Contract",
      workspaceSearch: true,
      entities: ["project"],
    });
    expect(results.results.project).toBeDefined();
    expect(results.results.issue).toBeUndefined();
    const projects = results.results.project ?? [];
    expect(projects.map((project) => project.identifier)).toContain("CT");
  });
});
