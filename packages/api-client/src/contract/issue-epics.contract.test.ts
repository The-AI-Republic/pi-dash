// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the epics collection, entry and children against live Django
// (NEWFRONT-984). An epic is an issue whose type carries is_epic; the
// views subclass the issue views, so behavior mirrors the issues
// collection with an epic-typed boundary. The archive and links mirrors
// stay unserved until a caller needs them.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import { createIssueType, listIssueTypes } from "../contracts/issue-types.js";
import {
  addEpicWorkItems,
  createEpic,
  deleteEpic,
  getEpic,
  listEpicWorkItems,
  listEpics,
  updateEpic,
} from "../contracts/epics.js";
import { listStates } from "../contracts/reference.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

const EPIC_PROBES = ["Contract epic one", "Contract epic two"];
const CHILD_PROBES = ["Contract epic child one", "Contract epic child two"];

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function clearProbes(client: ApiClient, projectId: string, names: string[]): Promise<void> {
  const listed = await listEpics(client, CONTRACT_WORKSPACE, projectId);
  for (const row of listed.results) {
    if (names.includes(row.name)) await deleteEpic(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

async function ensureEpicType(client: ApiClient, projectId: string): Promise<string> {
  const existing = await listIssueTypes(client, CONTRACT_WORKSPACE, projectId);
  const epic = existing.find((row) => row.is_epic);
  if (epic) return epic.id;
  const created = await createIssueType(client, CONTRACT_WORKSPACE, projectId, {
    name: "Contract Epic",
    is_epic: true,
  });
  return created.id;
}

describe.skipIf(!contractEnabled)("epic collection contract", () => {
  it("creates, reads, updates and deletes an epic", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, EPIC_PROBES);
    await ensureEpicType(client, projectId);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const todoState = states.find((state) => state.group === "unstarted");
    expect(todoState).toBeDefined();
    if (!todoState) return;

    const created = await createEpic(client, CONTRACT_WORKSPACE, projectId, {
      name: EPIC_PROBES[0] ?? "",
      state_id: todoState.id,
    });
    const detail = await getEpic(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(detail.name).toBe(EPIC_PROBES[0]);
    expect(detail.is_epic).toBe(true);

    const listed = await listEpics(client, CONTRACT_WORKSPACE, projectId);
    expect(listed.results.map((row) => row.id)).toContain(created.id);

    await updateEpic(client, CONTRACT_WORKSPACE, projectId, created.id, {
      name: EPIC_PROBES[1] ?? "",
    });
    const renamed = await getEpic(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(renamed.name).toBe(EPIC_PROBES[1]);

    await expect(
      updateEpic(client, CONTRACT_WORKSPACE, projectId, created.id, {
        type: "00000000-0000-0000-0000-000000000000",
      } as unknown as { name: string })
    ).rejects.toMatchObject({ status: 400 });

    await deleteEpic(client, CONTRACT_WORKSPACE, projectId, created.id);
    await expect(getEpic(client, CONTRACT_WORKSPACE, projectId, created.id)).rejects.toMatchObject({
      status: 404,
    });
  });

  it("keeps plain issues out of the collection", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await ensureEpicType(client, projectId);
    const plain = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: "Contract plain probe",
    });
    try {
      await expect(getEpic(client, CONTRACT_WORKSPACE, projectId, plain.id)).rejects.toMatchObject({
        status: 404,
      });
      await expect(deleteEpic(client, CONTRACT_WORKSPACE, projectId, plain.id)).rejects.toMatchObject({ status: 404 });
      const listed = await listEpics(client, CONTRACT_WORKSPACE, projectId);
      expect(listed.results.map((row) => row.id)).not.toContain(plain.id);
    } finally {
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, plain.id);
    }
  });

  it("attaches and lists children", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, EPIC_PROBES);
    await ensureEpicType(client, projectId);
    const epic = await createEpic(client, CONTRACT_WORKSPACE, projectId, {
      name: EPIC_PROBES[0] ?? "",
    });
    const childIds: string[] = [];
    for (const name of CHILD_PROBES) {
      const child = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name });
      childIds.push(child.id);
    }
    try {
      await addEpicWorkItems(client, CONTRACT_WORKSPACE, projectId, epic.id, childIds);
      const children = await listEpicWorkItems(client, CONTRACT_WORKSPACE, projectId, epic.id);
      expect(children.sub_issues.map((row) => row.id).sort()).toEqual([...childIds].sort());
    } finally {
      for (const id of childIds) await deleteIssue(client, CONTRACT_WORKSPACE, projectId, id);
      await deleteEpic(client, CONTRACT_WORKSPACE, projectId, epic.id);
    }
  });
});
