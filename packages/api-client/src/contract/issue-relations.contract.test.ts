// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: issue relations grouped read plus add and remove writes
// against live Django, including the reciprocal read semantics.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { addIssueRelations, listIssueRelations, removeIssueRelation } from "../contracts/issue-relations.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_A = "Contract relation probe A";
const PROBE_B = "Contract relation probe B";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function clearProbes(client: ApiClient, projectId: string): Promise<void> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of page.results.filter((entry) => entry.name === PROBE_A || entry.name === PROBE_B)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

async function freshPair(client: ApiClient, projectId: string): Promise<[string, string]> {
  await clearProbes(client, projectId);
  const first = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_A });
  const second = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_B });
  return [first.id, second.id];
}

describe.skipIf(!contractEnabled)("issue relations contract", () => {
  it("adds a symmetric edge and reads it back on both sides", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const [first, second] = await freshPair(client, projectId);

    const added = await addIssueRelations(client, CONTRACT_WORKSPACE, projectId, first, {
      relationType: "relates_to",
      issues: [second],
    });
    expect(added.map((row) => row.id)).toContain(second);

    const fromFirst = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, first);
    expect(fromFirst.relates_to.map((row) => row.id)).toContain(second);
    const fromSecond = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, second);
    expect(fromSecond.relates_to.map((row) => row.id)).toContain(first);

    await removeIssueRelation(client, CONTRACT_WORKSPACE, projectId, first, {
      relationType: "relates_to",
      relatedIssue: second,
    });
    const clearedFirst = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, first);
    expect(clearedFirst.relates_to.map((row) => row.id)).not.toContain(second);
    const clearedSecond = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, second);
    expect(clearedSecond.relates_to.map((row) => row.id)).not.toContain(first);

    await clearProbes(client, projectId);
  });

  it("reads a directed edge from each end under its own group", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const [first, second] = await freshPair(client, projectId);

    await addIssueRelations(client, CONTRACT_WORKSPACE, projectId, first, {
      relationType: "blocking",
      issues: [second],
    });
    const fromFirst = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, first);
    expect(fromFirst.blocking.map((row) => row.id)).toContain(second);
    const fromSecond = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, second);
    expect(fromSecond.blocked_by.map((row) => row.id)).toContain(first);

    await removeIssueRelation(client, CONTRACT_WORKSPACE, projectId, first, {
      relationType: "blocking",
      relatedIssue: second,
    });
    const cleared = await listIssueRelations(client, CONTRACT_WORKSPACE, projectId, first);
    expect(cleared.blocking).toHaveLength(0);

    await clearProbes(client, projectId);
  });
});
