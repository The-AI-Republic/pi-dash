// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: issue list and issue detail (read-only) against live Django.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { getIssue, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

const CONTRACT_ISSUE_NAME = "Parity quick-create probe";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("issues contract", () => {
  it("lists issues with the ungrouped envelope", async () => {
    const client = await signedIn();
    const page = await listIssues(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(page.total_count).toBeGreaterThanOrEqual(1);
    expect(page.results.length).toBeGreaterThanOrEqual(1);
    expect(page.results[0]?.project_id).toBe(await seededProjectId(client));
  });

  it("creates a work item and deletes it again", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    // A dead run may have left its row behind; clear it first so the
    // seed stays clean no matter how often this suite runs.
    const stale = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    for (const row of stale.results.filter((entry) => entry.name === CONTRACT_ISSUE_NAME)) {
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
    }
    const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: CONTRACT_ISSUE_NAME });
    expect(created.name).toBe(CONTRACT_ISSUE_NAME);
    expect(created.project_id).toBe(projectId);
    expect(created.sequence_id).toBeGreaterThan(0);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
  });

  it("reads one issue back by id", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    // Seeded by name: results[0] is newest-first, so under parallel runs
    // it is often a transient probe another file deletes mid-test.
    const first = page.results.find((row) => row.name === "Contract issue one");
    expect(first).toBeDefined();
    if (!first) return;
    const detail = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(detail.id).toBe(first.id);
    expect(detail.name).toBe(first.name);
  });
});
