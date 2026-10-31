// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: issue search with the parent, sub-issue and relation
// exclusion flags against live Django.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { addIssueRelations, removeIssueRelation } from "../contracts/issue-relations.js";
import { listIssues } from "../contracts/issues.js";
import { searchIssues } from "../contracts/issue-search.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_A = "Contract search probe A";
const PROBE_B = "Contract search probe B";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function issueIdByName(client: ApiClient, projectId: string, name: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const row = page.results.find((entry) => entry.name === name);
  expect(row).toBeDefined();
  return row?.id ?? "";
}

describe.skipIf(!contractEnabled)("issue search contract", () => {
  it("narrows by query and carries the picker columns", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);

    const hits = await searchIssues(client, CONTRACT_WORKSPACE, projectId, {
      search: "Contract sub-issue child",
    });
    const names = hits.map((hit) => hit.name).sort();
    expect(names).toEqual(["Contract sub-issue child A", "Contract sub-issue child B"]);
    expect(hits[0]?.project__identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    expect(hits[0]?.workspace__slug).toBe(CONTRACT_WORKSPACE);
    expect(typeof hits[0]?.sequence_id).toBe("number");
  });

  it("excludes the issue, its parent and its children for parent search", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const childA = await issueIdByName(client, projectId, "Contract sub-issue child A");
    const parent = await issueIdByName(client, projectId, "Contract sub-issue parent");
    const grandchild = await issueIdByName(client, projectId, "Contract sub-issue grandchild");

    const hits = await searchIssues(client, CONTRACT_WORKSPACE, projectId, { parent: true, issueId: childA });
    const ids = hits.map((hit) => hit.id);
    expect(ids).not.toContain(childA);
    expect(ids).not.toContain(parent);
    expect(ids).not.toContain(grandchild);
    // Unrelated roots stay eligible.
    expect(ids).toContain(await issueIdByName(client, projectId, "Contract issue one"));
  });

  it("lists root issues only for the add-existing picker", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const parent = await issueIdByName(client, projectId, "Contract sub-issue parent");
    const childA = await issueIdByName(client, projectId, "Contract sub-issue child A");

    const hits = await searchIssues(client, CONTRACT_WORKSPACE, projectId, { subIssue: true, issueId: parent });
    const ids = hits.map((hit) => hit.id);
    expect(ids).not.toContain(parent);
    expect(ids).not.toContain(childA);
    expect(ids).toContain(await issueIdByName(client, projectId, "Contract issue one"));
  });

  it("excludes already-related issues for the relation picker", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    for (const row of page.results.filter((entry) => entry.name === PROBE_A || entry.name === PROBE_B)) {
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
    }
    const first = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_A });
    const second = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_B });
    await addIssueRelations(client, CONTRACT_WORKSPACE, projectId, first.id, {
      relationType: "relates_to",
      issues: [second.id],
    });

    const hits = await searchIssues(client, CONTRACT_WORKSPACE, projectId, {
      issueRelation: true,
      issueId: first.id,
    });
    const ids = hits.map((hit) => hit.id);
    expect(ids).not.toContain(first.id);
    expect(ids).not.toContain(second.id);
    expect(ids).toContain(await issueIdByName(client, projectId, "Contract issue one"));

    await removeIssueRelation(client, CONTRACT_WORKSPACE, projectId, first.id, {
      relationType: "relates_to",
      relatedIssue: second.id,
    });
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, second.id);
  });
});
