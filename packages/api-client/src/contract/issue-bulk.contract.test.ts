// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: bulk issue delete and archive against live Django
// (NEWFRONT-611). Bulk update and bulk subscribe are cloud-only (no OSS
// route) and are proven by stub unit tests in contracts.test.ts instead.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { bulkArchiveIssues, bulkDeleteIssues } from "../contracts/issue-bulk.js";
import { createIssue, deleteIssue, updateIssue } from "../contracts/issue-actions.js";
import { restoreIssue } from "../contracts/issue-lifecycle.js";
import { getIssue, listArchivedIssues, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { listStates } from "../contracts/reference.js";
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

async function clearProbes(client: ApiClient, projectId: string, names: string[]): Promise<void> {
  const live = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of live.results.filter((entry) => names.includes(entry.name))) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
  const archived = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of archived.results.filter((entry) => names.includes(entry.name))) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

const BULK_DELETE_PROBES = ["Contract bulk delete one", "Contract bulk delete two"];

describe.skipIf(!contractEnabled)("bulk issue delete contract", () => {
  it("deletes every probe in one request", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_DELETE_PROBES);
    const created = [];
    for (const name of BULK_DELETE_PROBES) {
      created.push(await createIssue(client, CONTRACT_WORKSPACE, projectId, { name }));
    }
    const ids = created.map((issue) => issue.id);
    const result = await bulkDeleteIssues(client, CONTRACT_WORKSPACE, projectId, ids);
    expect(result.message).toContain("2");
    for (const id of ids) {
      await expect(getIssue(client, CONTRACT_WORKSPACE, projectId, id)).rejects.toMatchObject({ status: 404 });
    }
  });

  it("refuses an empty selection", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await expect(bulkDeleteIssues(client, CONTRACT_WORKSPACE, projectId, [])).rejects.toMatchObject({
      status: 400,
    });
  });
});

const BULK_ARCHIVE_PROBES = ["Contract bulk archive one", "Contract bulk archive two"];

describe.skipIf(!contractEnabled)("bulk issue archive contract", () => {
  it("archives every probe at once, then restores and deletes them", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_ARCHIVE_PROBES);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const doneState = states.find((state) => state.group === "completed");
    expect(doneState).toBeDefined();
    if (!doneState) return;
    const ids = [];
    for (const name of BULK_ARCHIVE_PROBES) {
      const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name });
      await updateIssue(client, CONTRACT_WORKSPACE, projectId, created.id, { state_id: doneState.id });
      ids.push(created.id);
    }
    const result = await bulkArchiveIssues(client, CONTRACT_WORKSPACE, projectId, ids);
    expect(result.archived_at).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    const archived = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId);
    for (const id of ids) expect(archived.results.map((row) => row.id)).toContain(id);
    for (const id of ids) {
      await restoreIssue(client, CONTRACT_WORKSPACE, projectId, id);
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, id);
    }
  });

  it("fails the batch when one issue is still open", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_ARCHIVE_PROBES);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const doneState = states.find((state) => state.group === "completed");
    expect(doneState).toBeDefined();
    if (!doneState) return;
    const open = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: BULK_ARCHIVE_PROBES[0] ?? "" });
    const done = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: BULK_ARCHIVE_PROBES[1] ?? "" });
    await updateIssue(client, CONTRACT_WORKSPACE, projectId, done.id, { state_id: doneState.id });
    await expect(bulkArchiveIssues(client, CONTRACT_WORKSPACE, projectId, [open.id, done.id])).rejects.toMatchObject({
      status: 400,
    });
    // The batch is all-or-nothing: the finished probe stayed live too.
    const archived = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId);
    expect(archived.results.map((row) => row.id)).not.toContain(done.id);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, open.id);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, done.id);
  });
});
