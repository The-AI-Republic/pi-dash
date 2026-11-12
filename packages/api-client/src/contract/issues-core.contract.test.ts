// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: full issue create/update, archive/restore, move, grouped and
// global lists against live Django (NEWFRONT-611).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { listCycles, transferCycleIssues } from "../contracts/cycles.js";
import { listProjectEstimates } from "../contracts/estimates.js";
import { listStates } from "../contracts/reference.js";
import { getMe } from "../contracts/users.js";
import {
  addIssueToCycle,
  createIssue,
  deleteIssue,
  removeIssueFromCycle,
  updateIssue,
} from "../contracts/issue-actions.js";
import { archiveIssue, moveIssueToProject, restoreIssue } from "../contracts/issue-lifecycle.js";
import {
  getIssue,
  listArchivedIssues,
  listGlobalIssues,
  listGroupedIssues,
  listIssues,
  listSubGroupedIssues,
} from "../contracts/issues.js";
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

async function secondProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === "CU");
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

async function seededIssueId(client: ApiClient, projectId: string, name: string): Promise<string> {
  const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  const seeded = page.results.find((row) => row.name === name);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/** Remove probe rows a dead run may have left behind (live + archived). */
async function clearProbes(client: ApiClient, projectId: string, name: string): Promise<void> {
  const live = await listIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of live.results.filter((entry) => entry.name === name)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
  const archived = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId);
  for (const row of archived.results.filter((entry) => entry.name === name)) {
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

const CORE_PROBE = "Contract core probe";

describe.skipIf(!contractEnabled)("full issue create/update contract", () => {
  it("creates with every field, patches every field, then deletes", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, CORE_PROBE);
    const parentId = await seededIssueId(client, projectId, "Contract issue one");
    const me = await getMe(client);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const startState = states.find((state) => state.group === "unstarted");
    const doneState = states.find((state) => state.group === "completed");
    expect(startState).toBeDefined();
    expect(doneState).toBeDefined();
    if (!startState || !doneState) return;
    const estimates = await listProjectEstimates(client, CONTRACT_WORKSPACE, projectId);
    const point = estimates.find((estimate) => estimate.name === "Contract estimate")?.points[0];
    expect(point).toBeDefined();
    if (!point) return;

    const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: CORE_PROBE,
      description_html: "<p>Core probe body</p>",
      state_id: startState.id,
      priority: "high",
      assignee_ids: [me.id],
      label_ids: [],
      estimate_point: point.id,
      start_date: "2026-01-05",
      target_date: "2026-01-20",
      parent_id: parentId,
      git_work_branch: "contract/core-probe_1.2",
    });
    expect(created.name).toBe(CORE_PROBE);
    expect(created.project_id).toBe(projectId);
    expect(created.sequence_id).toBeGreaterThan(0);
    expect(created.sort_order).toBeGreaterThan(0);
    expect(created.parent_id).toBe(parentId);

    const detail = await getIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(detail.state_id).toBe(startState.id);
    expect(detail.priority).toBe("high");
    expect(detail.assignee_ids).toContain(me.id);
    expect(detail.estimate_point).toBe(point.id);
    expect(detail.start_date).toBe("2026-01-05");
    expect(detail.target_date).toBe("2026-01-20");
    expect(detail.parent_id).toBe(parentId);
    expect(detail.description_html).toContain("Core probe body");
    expect(detail.git_work_branch).toBe("contract/core-probe_1.2");

    await updateIssue(client, CONTRACT_WORKSPACE, projectId, created.id, {
      name: `${CORE_PROBE} renamed`,
      description_html: "<p>Core probe body edited</p>",
      state_id: doneState.id,
      priority: "low",
      assignee_ids: [],
      estimate_point: null,
      start_date: "2026-02-01",
      target_date: "2026-02-10",
      parent_id: null,
      sort_order: 4242.5,
      git_work_branch: "contract/core-probe_1.3",
    });
    const patched = await getIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(patched.name).toBe(`${CORE_PROBE} renamed`);
    expect(patched.state_id).toBe(doneState.id);
    expect(patched.priority).toBe("low");
    expect(patched.assignee_ids).toEqual([]);
    expect(patched.estimate_point).toBeNull();
    expect(patched.start_date).toBe("2026-02-01");
    expect(patched.target_date).toBe("2026-02-10");
    expect(patched.parent_id).toBeNull();
    expect(patched.sort_order).toBe(4242.5);
    expect(patched.description_html).toContain("Core probe body edited");
    expect(patched.git_work_branch).toBe("contract/core-probe_1.3");

    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    await expect(getIssue(client, CONTRACT_WORKSPACE, projectId, created.id)).rejects.toMatchObject({
      status: 404,
    });
  });
});

const ARCHIVE_PROBE = "Contract archive probe";

describe.skipIf(!contractEnabled)("issue archive/restore contract", () => {
  it("archives, lists under archived, restores and deletes", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, ARCHIVE_PROBE);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const doneState = states.find((state) => state.group === "completed");
    expect(doneState).toBeDefined();
    if (!doneState) return;

    const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: ARCHIVE_PROBE });
    // Open states refuse the archive; only finished work archives.
    await expect(archiveIssue(client, CONTRACT_WORKSPACE, projectId, created.id)).rejects.toMatchObject({
      status: 400,
    });
    await updateIssue(client, CONTRACT_WORKSPACE, projectId, created.id, { state_id: doneState.id });
    const archived = await archiveIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(archived.archived_at).toMatch(/^\d{4}-\d{2}-\d{2}$/);

    const archivedList = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId);
    expect(archivedList.results.map((row) => row.id)).toContain(created.id);
    const liveList = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    expect(liveList.results.map((row) => row.id)).not.toContain(created.id);

    await restoreIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    const restored = await getIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(restored.archived_at).toBeNull();
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
  });
});

const MOVE_PROBE = "Contract move probe";

describe.skipIf(!contractEnabled)("issue move contract", () => {
  it("moves a probe to the second project and back by both ref styles", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const targetId = await secondProjectId(client);
    await clearProbes(client, projectId, MOVE_PROBE);
    await clearProbes(client, targetId, MOVE_PROBE);

    const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: MOVE_PROBE });
    const moved = await moveIssueToProject(client, CONTRACT_WORKSPACE, projectId, created.id, "CU");
    expect(moved.id).toBe(created.id);
    expect(moved.project_id).toBe(targetId);
    const movedDetail = await getIssue(client, CONTRACT_WORKSPACE, targetId, created.id);
    expect(movedDetail.name).toBe(MOVE_PROBE);
    await expect(getIssue(client, CONTRACT_WORKSPACE, projectId, created.id)).rejects.toMatchObject({
      status: 404,
    });

    const back = await moveIssueToProject(client, CONTRACT_WORKSPACE, targetId, created.id, projectId);
    expect(back.project_id).toBe(projectId);
    const homeDetail = await getIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(homeDetail.name).toBe(MOVE_PROBE);
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
  });
});

const TRANSFER_PROBE = "Contract transfer probe";

describe.skipIf(!contractEnabled)("cycle transfer-issues contract", () => {
  it("moves the probe to the target cycle and back", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, TRANSFER_PROBE);
    const cycles = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const source = cycles.find((row) => row.name === "Contract transfer source");
    const target = cycles.find((row) => row.name === "Contract cycle target");
    expect(source).toBeDefined();
    expect(target).toBeDefined();
    if (!source || !target) return;
    const created = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: TRANSFER_PROBE });
    await addIssueToCycle(client, CONTRACT_WORKSPACE, projectId, source.id, created.id);
    try {
      const moved = await transferCycleIssues(client, CONTRACT_WORKSPACE, projectId, source.id, target.id);
      expect(moved.message).toBe("Success");
      const transferred = await getIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(transferred.cycle_id).toBe(target.id);
      await transferCycleIssues(client, CONTRACT_WORKSPACE, projectId, target.id, source.id);
      const back = await getIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(back.cycle_id).toBe(source.id);
    } finally {
      const dropBridge = async (cycleId: string): Promise<void> => {
        try {
          await removeIssueFromCycle(client, CONTRACT_WORKSPACE, projectId, cycleId, created.id);
        } catch {
          // The bridge sits in the other cycle or is already gone.
        }
      };
      await dropBridge(source.id);
      await dropBridge(target.id);
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("refuses a missing destination cycle", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const cycles = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const source = cycles.find((row) => row.name === "Contract transfer source");
    expect(source).toBeDefined();
    if (!source) return;
    await expect(transferCycleIssues(client, CONTRACT_WORKSPACE, projectId, source.id, "")).rejects.toMatchObject({
      status: 400,
    });
  });
});

describe.skipIf(!contractEnabled)("grouped and filtered issue list contract", () => {
  it("groups by priority with per-group buckets", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listGroupedIssues(client, CONTRACT_WORKSPACE, projectId, { groupBy: "priority" });
    expect(page.grouped_by).toBe("priority");
    expect(page.sub_grouped_by).toBeNull();
    expect(page.total_results).toBeGreaterThanOrEqual(2);
    // The seeded rows are medium priority; the updates suite briefly
    // flips one of them, so only one medium row is guaranteed.
    const medium = page.results["medium"];
    expect(medium).toBeDefined();
    expect(medium?.total_results).toBeGreaterThanOrEqual(1);
    for (const row of medium?.results ?? []) expect(row.priority).toBe("medium");
  });

  it("groups by a to-many dimension with the raw join key", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listGroupedIssues(client, CONTRACT_WORKSPACE, projectId, { groupBy: "labels__id" });
    expect(page.grouped_by).toBe("labels__id");
    // The to-many join drops unlabeled rows from the total (only the
    // labeled seeded issue counts), so just one row is guaranteed.
    expect(page.total_results).toBeGreaterThanOrEqual(1);
  });

  it("sub-groups priority by state group with nested buckets", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listSubGroupedIssues(client, CONTRACT_WORKSPACE, projectId, {
      groupBy: "priority",
      subGroupBy: "state__group",
    });
    expect(page.grouped_by).toBe("priority");
    expect(page.sub_grouped_by).toBe("state__group");
    const medium = page.results["medium"];
    expect(medium).toBeDefined();
    expect(medium?.total_results).toBeGreaterThanOrEqual(1);
    const nested = medium?.results["backlog"];
    expect(nested).toBeDefined();
    expect(nested?.results.length).toBeGreaterThanOrEqual(1);
  });

  it("filters the flat list by priority", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId, { filters: { priority: "medium" } });
    expect(page.total_count).toBeGreaterThanOrEqual(1);
    expect(page.results.length).toBeGreaterThanOrEqual(1);
    for (const row of page.results) expect(row.priority).toBe("medium");
  });

  it("orders the flat list by sequence", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId, { orderBy: "sequence_id" });
    const sequences = page.results.map((row) => row.sequence_id);
    expect(sequences.length).toBeGreaterThanOrEqual(2);
    for (let index = 1; index < sequences.length; index += 1) {
      expect(sequences[index]).toBeGreaterThanOrEqual(sequences[index - 1] ?? 0);
    }
  });

  it("pages the flat list with perPage and cursor", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const first = await listIssues(client, CONTRACT_WORKSPACE, projectId, { perPage: 1 });
    expect(first.results).toHaveLength(1);
    expect(first.next_page_results).toBe(true);
    expect(first.next_cursor.length).toBeGreaterThan(0);
    const second = await listIssues(client, CONTRACT_WORKSPACE, projectId, {
      perPage: 1,
      cursor: first.next_cursor,
    });
    expect(second.results).toHaveLength(1);
    expect(second.results[0]?.id).not.toBe(first.results[0]?.id);
    expect(second.prev_page_results).toBe(true);
  });

  it("lists the workspace all-issues list across projects", async () => {
    const client = await signedIn();
    const page = await listGlobalIssues(client, CONTRACT_WORKSPACE);
    expect(page.total_results).toBeGreaterThanOrEqual(2);
    const names = page.results.map((row) => row.name);
    expect(names).toContain("Contract issue one");
    expect(names).toContain("Contract issue two");
    expect(page.results[0]?.state__group).toBeDefined();
    const one = await listGlobalIssues(client, CONTRACT_WORKSPACE, { perPage: 1 });
    expect(one.results).toHaveLength(1);
    expect(one.next_page_results).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("issue executor pin contract", () => {
  it("reads the executor pin off the issue detail", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const issueId = await seededIssueId(client, projectId, "Contract issue one");
    const row = await getIssue(client, CONTRACT_WORKSPACE, projectId, issueId);
    expect(row.agent_executor === null || typeof row.agent_executor === "string").toBe(true);
  });
});
