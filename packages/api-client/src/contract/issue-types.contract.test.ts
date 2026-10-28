// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: project work-item types against live Django (NEWFRONT-984).
// Types are workspace rows linked onto projects; the list answers the
// project's links. Creation is admin-only, like labels.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { createIssueType, listIssueTypes } from "../contracts/issue-types.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

describe.skipIf(!contractEnabled)("issue types contract", () => {
  it("creates a type and lists it on the project", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const stamp = Date.now();
    const created = await createIssueType(client, CONTRACT_WORKSPACE, projectId, {
      name: `Contract spike ${stamp}`,
      description: "Time-boxed research",
    });
    expect(created.name).toBe(`Contract spike ${stamp}`);
    expect(created.is_epic).toBe(false);

    const epic = await createIssueType(client, CONTRACT_WORKSPACE, projectId, {
      name: `Contract epic type ${stamp}`,
      is_epic: true,
    });
    expect(epic.is_epic).toBe(true);

    const listed = await listIssueTypes(client, CONTRACT_WORKSPACE, projectId);
    const names = listed.map((row) => row.name);
    expect(names).toContain(`Contract spike ${stamp}`);
    expect(names).toContain(`Contract epic type ${stamp}`);
  });

  it("refuses a blank name", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await expect(createIssueType(client, CONTRACT_WORKSPACE, projectId, { name: "   " })).rejects.toMatchObject({
      status: 400,
    });
  });
});
