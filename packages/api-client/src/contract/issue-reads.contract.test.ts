// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: issue identity reads (short-link meta, lite identifier
// fetch), the execution-target pod list, and the cycle-list status leg
// the cycle picker filters on. All read-only against seeded rows.
import { describe, expect, it } from "vitest";
import { listCycles } from "../contracts/cycles.js";
import { getIssueByIdentifier, getIssueMeta } from "../contracts/issue-identity.js";
import { listIssues } from "../contracts/issues.js";
import { listPods } from "../contracts/pods.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededIssue(client: ApiClient, projectId: string): Promise<{ id: string; sequence_id: number }> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const first = page.results[0];
  expect(first).toBeDefined();
  return { id: first?.id ?? "", sequence_id: first?.sequence_id ?? 0 };
}

describe.skipIf(!contractEnabled)("issue identity contract", () => {
  it("resolves short-link meta and the lite identifier row", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const issue = await seededIssue(client, projectId);
    const meta = await getIssueMeta(client, CONTRACT_WORKSPACE, projectId, issue.id);
    expect(meta.project_identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    expect(meta.sequence_id).toBe(issue.sequence_id);
    const lite = await getIssueByIdentifier(client, CONTRACT_WORKSPACE, CONTRACT_PROJECT_IDENTIFIER, meta.sequence_id);
    expect(lite.id).toBe(issue.id);
    expect(lite.project_id).toBe(projectId);
    expect(lite.sequence_id).toBe(issue.sequence_id);
  });
});

describe.skipIf(!contractEnabled)("execution pod list contract", () => {
  it("lists the project's pods default-first", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const pods = await listPods(client, { projectId });
    expect(pods.length).toBeGreaterThan(0);
    expect(pods[0]?.is_default).toBe(true);
    expect(pods.every((pod) => pod.project === projectId)).toBe(true);
    expect(pods.every((pod) => pod.project_identifier === CONTRACT_PROJECT_IDENTIFIER)).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("cycle status contract", () => {
  it("carries the lifecycle marker the picker filters on", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const cycles = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const seeded = cycles.find((cycle) => cycle.name === "Contract cycle");
    expect(seeded).toBeDefined();
    expect(typeof seeded?.status).toBe("string");
    expect((seeded?.status ?? "").length).toBeGreaterThan(0);
  });
});
