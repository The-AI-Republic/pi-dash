// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: worklog entries log, list, read, edit and delete against
// live Django (ISS-206, cloud leg). The seed user is a workspace admin
// on the seeded project, hence always eligible to log; the
// member/guest/intake edges stay covered by the Django suite.
import { describe, expect, it } from "vitest";
import { createIssue, deleteIssue } from "../contracts/issue-actions.js";
import {
  deleteWorklogEntry,
  getWorklogEntry,
  listWorklogEntries,
  logWorklogEntry,
  updateWorklogEntry,
} from "../contracts/issue-worklog.js";
import { listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

const PROBE_NAME = "Contract worklog probe";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function freshProbe(client: ApiClient, projectId: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of page.results.filter((entry) => entry.name === PROBE_NAME)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
  const probe = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_NAME });
  return probe.id;
}

describe.skipIf(!contractEnabled)("issue worklog contract", () => {
  it("logs, lists newest first, reads, edits and deletes an entry", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    const first = await logWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, {
      minutes: 30,
      logged_on: "2026-10-19",
    });
    expect(first.minutes).toBe(30);
    expect(first.note).toBe("");
    expect(first.actor_detail?.display_name).toBeDefined();

    const second = await logWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, {
      minutes: 90,
      logged_on: "2026-10-20",
      note: "Deep work",
    });
    expect(second.note).toBe("Deep work");
    expect(second.issue).toBe(probeId);

    const listed = await listWorklogEntries(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(listed.map((row) => row.id)).toEqual([second.id, first.id]);

    const read = await getWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, first.id);
    expect(read.minutes).toBe(30);

    const edited = await updateWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, first.id, {
      minutes: 45,
      note: "Still going",
    });
    expect(edited.minutes).toBe(45);
    expect(edited.note).toBe("Still going");

    await deleteWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, first.id);
    await deleteWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, second.id);
    const cleared = await listWorklogEntries(client, CONTRACT_WORKSPACE, projectId, probeId);
    expect(cleared).toEqual([]);

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });

  it("rejects non-positive minutes with 400", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    const probeId = await freshProbe(client, projectId);

    await expect(
      logWorklogEntry(client, CONTRACT_WORKSPACE, projectId, probeId, {
        minutes: 0,
        logged_on: "2026-10-20",
      })
    ).rejects.toMatchObject({ status: 400 });

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
  });
});
