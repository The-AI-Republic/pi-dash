// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: issue list and issue detail (read-only) against live Django.
import { describe, expect, it } from "vitest";
import { getIssue, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { signIn } from "../contracts/auth.js";
import {
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
} from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  const client = contractClient();
  const result = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  return client;
}

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

  it("reads one issue back by id", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    const detail = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(detail.id).toBe(first.id);
    expect(detail.name).toBe(first.name);
  });
});
