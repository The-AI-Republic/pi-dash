// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: archives reads and archive/restore writes against live
// Django (NEWFRONT-697): the archived-issues list query, the
// archive-scoped issue read, archived cycle/module lists and details,
// and cycle/module archive/restore.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { archiveCycle, getArchivedCycle, listArchivedCycles, listCycles, restoreCycle } from "../contracts/cycles.js";
import { createIssue, deleteIssue, updateIssue } from "../contracts/issue-actions.js";
import { archiveIssue, restoreIssue } from "../contracts/issue-lifecycle.js";
import {
  getArchivedIssue,
  listArchivedIssues,
  listGroupedArchivedIssues,
  listIssues,
  listSubGroupedArchivedIssues,
} from "../contracts/issues.js";
import {
  archiveModule,
  getArchivedModule,
  listArchivedModules,
  listModules,
  restoreModule,
} from "../contracts/modules.js";
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

const URGENT_PROBE = "Contract archives urgent probe";
const LOW_PROBE = "Contract archives low probe";

describe.skipIf(!contractEnabled)("archived issues list query contract", () => {
  // This contract seeds, archives, queries and cleans up several issue
  // probes; a cold or loaded backend takes longer than the package-wide
  // 30s budget even though every request succeeds.
  it("filters, orders, pages, groups and reads one archived issue", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbes(client, projectId, URGENT_PROBE);
    await clearProbes(client, projectId, LOW_PROBE);
    const states = await listStates(client, CONTRACT_WORKSPACE, projectId);
    const doneState = states.find((state) => state.group === "completed");
    expect(doneState).toBeDefined();
    if (!doneState) return;

    const urgent = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: URGENT_PROBE,
      priority: "urgent",
    });
    const low = await createIssue(client, CONTRACT_WORKSPACE, projectId, {
      name: LOW_PROBE,
      priority: "low",
    });
    await updateIssue(client, CONTRACT_WORKSPACE, projectId, urgent.id, { state_id: doneState.id });
    await updateIssue(client, CONTRACT_WORKSPACE, projectId, low.id, { state_id: doneState.id });
    await archiveIssue(client, CONTRACT_WORKSPACE, projectId, urgent.id);
    await archiveIssue(client, CONTRACT_WORKSPACE, projectId, low.id);

    try {
      // The rich expression rides `filters` as a JSON string.
      const filtered = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId, {
        filters: { filters: JSON.stringify({ and: [{ priority__in: "urgent" }] }) },
      });
      const filteredIds = filtered.results.map((row) => row.id);
      expect(filteredIds).toContain(urgent.id);
      expect(filteredIds).not.toContain(low.id);

      const ordered = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId, {
        orderBy: "-created_at",
      });
      expect(ordered.results.map((row) => row.id)).toEqual(expect.arrayContaining([urgent.id, low.id]));

      // Two probes guarantee a second page at one row per page.
      const first = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId, { perPage: 1 });
      expect(first.results).toHaveLength(1);
      expect(first.next_page_results).toBe(true);
      const second = await listArchivedIssues(client, CONTRACT_WORKSPACE, projectId, {
        perPage: 1,
        cursor: first.next_cursor,
      });
      expect(second.results).toHaveLength(1);
      expect(second.results[0]?.id).not.toBe(first.results[0]?.id);

      const grouped = await listGroupedArchivedIssues(client, CONTRACT_WORKSPACE, projectId, {
        groupBy: "priority",
      });
      expect(grouped.grouped_by).toBe("priority");
      expect(grouped.results["urgent"]?.results.map((row) => row.id)).toContain(urgent.id);

      const subGrouped = await listSubGroupedArchivedIssues(client, CONTRACT_WORKSPACE, projectId, {
        groupBy: "priority",
        subGroupBy: "state__group",
      });
      const urgentBucket = subGrouped.results["urgent"]?.results["completed"];
      expect(urgentBucket?.results.map((row) => row.id)).toContain(urgent.id);

      const detail = await getArchivedIssue(client, CONTRACT_WORKSPACE, projectId, urgent.id);
      expect(detail.id).toBe(urgent.id);
      expect(detail.name).toBe(URGENT_PROBE);
      expect(detail.archived_at).not.toBeNull();
    } finally {
      await restoreIssue(client, CONTRACT_WORKSPACE, projectId, urgent.id).catch(() => undefined);
      await restoreIssue(client, CONTRACT_WORKSPACE, projectId, low.id).catch(() => undefined);
      await clearProbes(client, projectId, URGENT_PROBE);
      await clearProbes(client, projectId, LOW_PROBE);
    }
  }, 60_000);
});

describe.skipIf(!contractEnabled)("archived cycles contract", () => {
  it("lists archived cycles and reads one detail", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const rows = await listArchivedCycles(client, CONTRACT_WORKSPACE, projectId);
    const seeded = rows.find((row) => row.name === "Contract archived cycle");
    expect(seeded).toBeDefined();
    if (!seeded) return;
    expect(seeded.project_id).toBe(projectId);
    expect(seeded.status).toBe("COMPLETED");
    expect(seeded.archived_at).not.toBeNull();
    expect(Array.isArray(seeded.assignee_ids)).toBe(true);

    const detail = await getArchivedCycle(client, CONTRACT_WORKSPACE, projectId, seeded.id);
    expect(detail.id).toBe(seeded.id);
    expect(detail.name).toBe("Contract archived cycle");
    expect(detail.distribution.assignees).toBeDefined();
    expect(detail.distribution.labels).toBeDefined();
  });

  it("refuses an unfinished cycle, archives and restores a finished one", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const live = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const upcoming = live.find((row) => row.name === "Contract upcoming cycle");
    const roundTrip = live.find((row) => row.name === "Contract archive cycle");
    // A dead run may have left the round-trip cycle archived; heal it.
    if (!roundTrip) {
      const archived = await listArchivedCycles(client, CONTRACT_WORKSPACE, projectId);
      const stuck = archived.find((row) => row.name === "Contract archive cycle");
      expect(stuck).toBeDefined();
      if (stuck) await restoreCycle(client, CONTRACT_WORKSPACE, projectId, stuck.id);
    }
    const fresh = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const target = fresh.find((row) => row.name === "Contract archive cycle");
    expect(upcoming).toBeDefined();
    expect(target).toBeDefined();
    if (!upcoming || !target) return;

    await expect(archiveCycle(client, CONTRACT_WORKSPACE, projectId, upcoming.id)).rejects.toMatchObject({
      status: 400,
    });

    const stamp = await archiveCycle(client, CONTRACT_WORKSPACE, projectId, target.id);
    expect(typeof stamp.archived_at).toBe("string");
    const archived = await listArchivedCycles(client, CONTRACT_WORKSPACE, projectId);
    expect(archived.map((row) => row.id)).toContain(target.id);
    const liveAfter = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    expect(liveAfter.map((row) => row.id)).not.toContain(target.id);

    await expect(restoreCycle(client, CONTRACT_WORKSPACE, projectId, target.id)).resolves.toBe(204);
    const liveRestored = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    expect(liveRestored.map((row) => row.id)).toContain(target.id);
  });
});

describe.skipIf(!contractEnabled)("archived modules contract", () => {
  it("lists archived modules and reads one detail", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const rows = await listArchivedModules(client, CONTRACT_WORKSPACE, projectId);
    const seeded = rows.find((row) => row.name === "Contract archived module");
    expect(seeded).toBeDefined();
    if (!seeded) return;
    expect(seeded.project_id).toBe(projectId);
    expect(seeded.status).toBe("completed");
    expect(seeded.lead_id).not.toBeNull();
    expect(seeded.member_ids.length).toBeGreaterThanOrEqual(1);
    expect(seeded.start_date).toMatch(/^\d{4}-\d{2}-\d{2}$/);

    const detail = await getArchivedModule(client, CONTRACT_WORKSPACE, projectId, seeded.id);
    expect(detail.id).toBe(seeded.id);
    expect(detail.name).toBe("Contract archived module");
    expect(Array.isArray(detail.link_module)).toBe(true);
    expect(detail.distribution.assignees).toBeDefined();
    expect(detail.distribution.labels).toBeDefined();
  });

  it("refuses an unfinished module, archives and restores a finished one", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const live = await listModules(client, CONTRACT_WORKSPACE, projectId);
    const planned = live.find((row) => row.name === "Contract planned module");
    let target = live.find((row) => row.name === "Contract archive module");
    // A dead run may have left the round-trip module archived; heal it.
    if (!target) {
      const archived = await listArchivedModules(client, CONTRACT_WORKSPACE, projectId);
      const stuck = archived.find((row) => row.name === "Contract archive module");
      expect(stuck).toBeDefined();
      if (stuck) await restoreModule(client, CONTRACT_WORKSPACE, projectId, stuck.id);
      const fresh = await listModules(client, CONTRACT_WORKSPACE, projectId);
      target = fresh.find((row) => row.name === "Contract archive module");
    }
    expect(planned).toBeDefined();
    expect(target).toBeDefined();
    if (!planned || !target) return;

    await expect(archiveModule(client, CONTRACT_WORKSPACE, projectId, planned.id)).rejects.toMatchObject({
      status: 400,
    });

    const stamp = await archiveModule(client, CONTRACT_WORKSPACE, projectId, target.id);
    expect(typeof stamp.archived_at).toBe("string");
    const archived = await listArchivedModules(client, CONTRACT_WORKSPACE, projectId);
    expect(archived.map((row) => row.id)).toContain(target.id);
    const liveAfter = await listModules(client, CONTRACT_WORKSPACE, projectId);
    expect(liveAfter.map((row) => row.id)).not.toContain(target.id);

    await expect(restoreModule(client, CONTRACT_WORKSPACE, projectId, target.id)).resolves.toBe(204);
    const liveRestored = await listModules(client, CONTRACT_WORKSPACE, projectId);
    expect(liveRestored.map((row) => row.id)).toContain(target.id);
  });
});
