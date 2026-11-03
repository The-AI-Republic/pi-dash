// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: bulk issue delete, archive, batch dates, update and
// subscribe against live Django (NEWFRONT-611, NEWFRONT-984). The update
// and subscribe routes ride the OSS stack API ungated — the cloud gate
// lives in the web_new edition module, not here.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  bulkArchiveIssues,
  bulkDeleteIssues,
  bulkSubscribeIssues,
  bulkUpdateIssueDates,
  bulkUpdateIssues,
} from "../contracts/issue-bulk.js";
import { createIssue, deleteIssue, updateIssue } from "../contracts/issue-actions.js";
import { restoreIssue } from "../contracts/issue-lifecycle.js";
import { getIssue, listArchivedIssues, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { listStates } from "../contracts/reference.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_SECONDARY_EMAIL,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";

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

const BULK_DATES_PROBES = ["Contract bulk dates one", "Contract bulk dates two"];

describe.skipIf(!contractEnabled)("bulk issue dates contract", () => {
  it("updates every probe's dates in one request", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_DATES_PROBES);
    const first = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: BULK_DATES_PROBES[0] ?? "",
      start_date: "2026-01-05",
      target_date: "2026-01-20",
    });
    const second = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: BULK_DATES_PROBES[1] ?? "",
      start_date: "2026-01-05",
      target_date: "2026-01-20",
    });
    const result = await bulkUpdateIssueDates(client, CONTRACT_WORKSPACE, projectId, [
      { id: first.id, start_date: "2026-02-01", target_date: "2026-02-10" },
      // A partial row keeps the stored start date.
      { id: second.id, target_date: "2026-03-15" },
    ]);
    expect(result.message).toContain("successfully");
    const firstDetail = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(firstDetail.start_date).toBe("2026-02-01");
    expect(firstDetail.target_date).toBe("2026-02-10");
    const secondDetail = await getIssue(client, CONTRACT_WORKSPACE, projectId, second.id);
    expect(secondDetail.start_date).toBe("2026-01-05");
    expect(secondDetail.target_date).toBe("2026-03-15");
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, second.id);
  });

  it("refuses a batch whose start passes its target", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_DATES_PROBES);
    const probe = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: BULK_DATES_PROBES[0] ?? "",
      start_date: "2026-03-01",
      target_date: "2026-03-10",
    });
    await expect(
      bulkUpdateIssueDates(client, CONTRACT_WORKSPACE, projectId, [{ id: probe.id, start_date: "2026-04-01" }])
    ).rejects.toMatchObject({ status: 400 });
    // The batch is all-or-nothing: the stored dates are untouched.
    const detail = await getIssue(client, CONTRACT_WORKSPACE, projectId, probe.id);
    expect(detail.start_date).toBe("2026-03-01");
    expect(detail.target_date).toBe("2026-03-10");
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probe.id);
  });
});

const BULK_OPERATION_PROBES = ["Contract bulk operation one", "Contract bulk operation two"];

describe.skipIf(!contractEnabled)("bulk issue operation contract", () => {
  it("applies one properties object across the selection", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_OPERATION_PROBES);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const doneState = states.find((state) => state.group === "completed");
    expect(doneState).toBeDefined();
    if (!doneState) return;
    const ids = [];
    for (const name of BULK_OPERATION_PROBES) {
      const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name });
      ids.push(created.id);
    }
    const result = await bulkUpdateIssues(client, CONTRACT_WORKSPACE, projectId, {
      issue_ids: ids,
      properties: {
        state_id: doneState.id,
        priority: "urgent",
        start_date: "2026-05-01",
        target_date: "2026-05-10",
      },
    });
    expect(result.updated).toBe(2);
    for (const id of ids) {
      const detail = await getIssue(client, CONTRACT_WORKSPACE, projectId, id);
      expect(detail.state_id).toBe(doneState.id);
      expect(detail.priority).toBe("urgent");
      expect(detail.start_date).toBe("2026-05-01");
      expect(detail.target_date).toBe("2026-05-10");
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, id);
    }
  });

  it("refuses unknown properties and empty selections", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_OPERATION_PROBES);
    const probe = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: BULK_OPERATION_PROBES[0] ?? "",
    });
    await expect(
      bulkUpdateIssues(client, CONTRACT_WORKSPACE, projectId, {
        issue_ids: [probe.id],
        properties: { name: "renamed" } as unknown as { priority: "high" },
      })
    ).rejects.toMatchObject({ status: 400 });
    await expect(
      bulkUpdateIssues(client, CONTRACT_WORKSPACE, projectId, {
        issue_ids: [],
        properties: { priority: "high" },
      })
    ).rejects.toMatchObject({ status: 400 });
    // The failed batches wrote nothing.
    const detail = await getIssue(client, CONTRACT_WORKSPACE, projectId, probe.id);
    expect(detail.priority).not.toBe("high");
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probe.id);
  });
});

const BULK_SUBSCRIBE_PROBES = ["Contract bulk subscribe one", "Contract bulk subscribe two"];

describe.skipIf(!contractEnabled)("bulk issue subscribe contract", () => {
  it("subscribes once and repeats idempotently", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, BULK_SUBSCRIBE_PROBES);
    const ids = [];
    for (const name of BULK_SUBSCRIBE_PROBES) {
      const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name });
      ids.push(created.id);
    }
    // The notifications worker subscribes the issue creator in the
    // background, so the creator's own count races it. The secondary
    // member is never auto-subscribed: their count is deterministic.
    const secondary = contractClient();
    const signInResult = await signIn(secondary, {
      email: CONTRACT_SECONDARY_EMAIL,
      password: CONTRACT_PASSWORD,
    });
    expect(signInResult.ok).toBe(true);
    const first = await bulkSubscribeIssues(secondary, CONTRACT_WORKSPACE, projectId, ids);
    expect(first.subscribed).toBe(2);
    const repeat = await bulkSubscribeIssues(secondary, CONTRACT_WORKSPACE, projectId, ids);
    expect(repeat.subscribed).toBe(0);
    for (const id of ids) await deleteIssue(client, CONTRACT_WORKSPACE, projectId, id);
  });

  it("refuses an empty selection", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await expect(bulkSubscribeIssues(client, CONTRACT_WORKSPACE, projectId, [])).rejects.toMatchObject({
      status: 400,
    });
  });
});
