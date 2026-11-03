// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: description-version history list and single-version read
// against live Django, over the seeded version row.
import { describe, expect, it } from "vitest";
import { getDescriptionVersion, listDescriptionVersions } from "../contracts/issue-versions.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function firstIssueId(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const first = page.results.find((row) => row.name === "Contract issue one");
  expect(first).toBeDefined();
  return first?.id ?? "";
}

describe.skipIf(!contractEnabled)("description versions contract", () => {
  it("lists the history and reads one version back", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issueId = await firstIssueId(client, projectId);

    const page = await listDescriptionVersions(client, CONTRACT_WORKSPACE, projectId, issueId);
    expect(page.total_results).toBeGreaterThanOrEqual(1);
    expect(page.results.length).toBeGreaterThanOrEqual(1);
    expect(page.results[0]?.issue).toBe(issueId);

    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    const detail = await getDescriptionVersion(client, CONTRACT_WORKSPACE, projectId, issueId, first.id);
    expect(detail.id).toBe(first.id);
    expect(detail.description_html).toContain("Contract version probe");
  });
});
