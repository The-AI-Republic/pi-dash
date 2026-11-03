// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the issue activity feed against live Django. Rows are seeded
// directly (live rows are written by celery workers, which do not run in
// contract tests); the suite proves the list shape plus the refresh
// cursor. Filter dimensions and sort are client-side.
import { describe, expect, it } from "vitest";
import { listIssueActivities } from "../contracts/issue-activity.js";
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

/** The seeded issue carrying the seeded activity rows. */
async function seededIssueId(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const seeded = page.results.find((issue) => issue.name === "Contract issue one");
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("issue activity contract", () => {
  it("lists the seeded rows with their field changes", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issueId = await seededIssueId(client, projectId);
    const activities = await listIssueActivities(client, CONTRACT_WORKSPACE, projectId, issueId);
    const stateChange = activities.find((row) => row.verb === "updated" && row.field === "state");
    expect(stateChange).toBeDefined();
    expect(stateChange?.old_value).toBe("Triage");
    expect(stateChange?.new_value).toBe("In Progress");
    expect(stateChange?.actor_detail?.display_name).toBe("contract.tester");
    expect(activities.some((row) => row.verb === "created")).toBe(true);
  });

  it("narrows the feed with the created-after cursor", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issueId = await seededIssueId(client, projectId);
    const future = await listIssueActivities(client, CONTRACT_WORKSPACE, projectId, issueId, {
      createdAfter: "2999-01-01T00:00:00Z",
    });
    expect(future).toEqual([]);
    const past = await listIssueActivities(client, CONTRACT_WORKSPACE, projectId, issueId, {
      createdAfter: "2000-01-01T00:00:00Z",
    });
    expect(past.length).toBeGreaterThan(0);
  });
});
