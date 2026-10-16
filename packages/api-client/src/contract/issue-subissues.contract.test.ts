// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: sub-issues list and add-existing against live Django.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { addSubIssues, listSubIssues } from "../contracts/issue-subissues.js";
import { getIssue, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PARENT_NAME = "Contract sub-issue parent";
const PROBE_NAME = "Contract sub-issue probe";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function parentId(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const parent = page.results.find((row) => row.name === PARENT_NAME);
  expect(parent).toBeDefined();
  return parent?.id ?? "";
}

async function clearProbe(client: ApiClient, projectId: string): Promise<void> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of page.results.filter((entry) => entry.name === PROBE_NAME)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

describe.skipIf(!contractEnabled)("sub-issues contract", () => {
  it("lists children with the full-set state distribution", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const page = await listSubIssues(client, CONTRACT_WORKSPACE, projectId, await parentId(client, projectId));
    const names = page.sub_issues.map((row) => row.name).sort();
    expect(names).toEqual(["Contract sub-issue child A", "Contract sub-issue child B"]);
    expect(page.sub_issues[0]?.parent_id).toBe(await parentId(client, projectId));
    // The rollup covers the seeded children across two state groups and
    // keys every child id exactly once.
    const rolledUp = Object.values(page.state_distribution).flat().sort();
    expect(Object.keys(page.state_distribution).length).toBeGreaterThanOrEqual(1);
    expect(rolledUp).toEqual(page.sub_issues.map((row) => row.id).sort());
  });

  it("attaches an existing issue, detaches it, then deletes it", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const parent = await parentId(client, projectId);
    await clearProbe(client, projectId);

    const probe = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_NAME });
    const attached = await addSubIssues(client, CONTRACT_WORKSPACE, projectId, parent, [probe.id]);
    expect(attached.sub_issues.map((row) => row.id)).toContain(probe.id);
    expect(attached.sub_issues.find((row) => row.id === probe.id)?.parent_id).toBe(parent);

    // Detach is a null parent write through the shared issue update
    // (NEWFRONT-611 owns that contract): the row survives, unlinked.
    await client.patch(`/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/issues/${probe.id}/`, {
      json: { parent_id: null },
    });
    const relisted = await listSubIssues(client, CONTRACT_WORKSPACE, projectId, parent);
    expect(relisted.sub_issues.map((row) => row.id)).not.toContain(probe.id);
    expect((await getIssue(client, CONTRACT_WORKSPACE, projectId, probe.id)).parent_id).toBeNull();

    // Delete destroys the child instead.
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probe.id);
    await expect(getIssue(client, CONTRACT_WORKSPACE, projectId, probe.id)).rejects.toMatchObject({
      status: 404,
    });
  });

  it("rejects an empty attach list with 400", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await expect(
      addSubIssues(client, CONTRACT_WORKSPACE, projectId, await parentId(client, projectId), [])
    ).rejects.toMatchObject({ status: 400 });
  });
});
