// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the full cycles surface against live Django (NEWFRONT-706).
// Every write runs on a probe cycle created for this suite and removed in
// the finally block, so the seeded "Contract cycle" stays untouched for
// the thinner suites that read it. The workspace active-cycles and
// detailed cycle-progress reads are cloud-only (no OSS route), so they are
// proven by unit stubs in src/contracts/cycles.test.ts; the note at the
// end of the runbook table records where their live proof comes from.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  addCycleFavorite,
  archiveCycle,
  checkCycleDates,
  createCycle,
  deleteCycle,
  getArchivedCycle,
  getCycle,
  getCycleAnalytics,
  getCycleProgress,
  listArchivedCycles,
  listCycleRows,
  listCycles,
  listWorkspaceCycles,
  removeCycleFavorite,
  restoreCycle,
  transferCycleIssues,
  updateCycle,
} from "../contracts/cycles.js";
import { listCycleIssues } from "../contracts/issues.js";
import { addIssueToCycle, createIssue, deleteIssue, removeIssueFromCycle } from "../contracts/issue-actions.js";
import { listProjects } from "../contracts/projects.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";

const PROBE = "Contract cycles probe";
const PROBE_ISSUE = "Contract cycles probe item";

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/** Remove a probe cycle from an earlier crashed run, if it survived. */
async function clearProbeCycle(client: ApiClient, projectId: string): Promise<void> {
  const rows = await listCycleRows(client, CONTRACT_WORKSPACE, projectId);
  for (const row of rows.filter((candidate) => candidate.name === PROBE)) {
    await deleteCycle(client, CONTRACT_WORKSPACE, projectId, row.id);
  }
}

/**
 * `offsetDays` from today in UTC, as the API's datetime serializer reads
 * it. The range is generous so the probe is current on both sides of the
 * project timezone's midnight.
 */
function instant(offsetDays: number): string {
  const date = new Date();
  date.setUTCDate(date.getUTCDate() + offsetDays);
  date.setUTCHours(12, 0, 0, 0);
  return date.toISOString();
}

/** yyyy-MM-dd for the date-check, which takes date-only values. */
function calendarDate(offsetDays: number): string {
  return instant(offsetDays).slice(0, 10);
}

describe.skipIf(!contractEnabled)("cycles list and detail contract", () => {
  it("lists full rows, filters to the current cycle, and reads detail", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      description: "probe description",
      start_date: instant(-1),
      end_date: instant(1),
    });
    try {
      expect(created.name).toBe(PROBE);
      expect(created.status).toBe("CURRENT");
      expect(created.total_issues).toBe(0);

      const rows = await listCycleRows(client, CONTRACT_WORKSPACE, projectId);
      const probe = rows.find((row) => row.id === created.id);
      expect(probe).toBeDefined();
      expect(probe?.is_favorite).toBe(false);
      expect(probe?.assignee_ids).toEqual([]);
      expect(probe?.end_date).toContain("T");

      const current = await listCycleRows(client, CONTRACT_WORKSPACE, projectId, { cycleView: "current" });
      expect(current.map((row) => row.id)).toContain(created.id);

      const detail = await getCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(detail.id).toBe(created.id);
      expect(detail.description).toBe("probe description");
      expect(detail.sub_issues ?? 0).toBe(0);

      const thin = await listCycles(client, CONTRACT_WORKSPACE, projectId);
      expect(thin.find((row) => row.id === created.id)?.name).toBe(PROBE);

      const workspaceRows = await listWorkspaceCycles(client, CONTRACT_WORKSPACE);
      const workspaceProbe = workspaceRows.find((row) => row.id === created.id);
      expect(workspaceProbe).toBeDefined();
      expect(workspaceProbe?.name).toBe(PROBE);
      expect(workspaceProbe?.total_issues).toBe(0);
      expect("is_favorite" in (workspaceProbe ?? {})).toBe(false);
      expect("status" in (workspaceProbe ?? {})).toBe(false);
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("refuses a create whose start date is after its end date", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await expect(
      createCycle(client, CONTRACT_WORKSPACE, projectId, {
        name: `${PROBE} invalid`,
        start_date: instant(2),
        end_date: instant(1),
      })
    ).rejects.toMatchObject({ status: 400 });
  });

  it("updates the probe and answers the echo", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-1),
      end_date: instant(1),
    });
    try {
      const renamed = await updateCycle(client, CONTRACT_WORKSPACE, projectId, created.id, {
        name: `${PROBE} renamed`,
      });
      expect(renamed.id).toBe(created.id);
      expect(renamed.name).toBe(`${PROBE} renamed`);
      expect(renamed.start_date).toBeTruthy();
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });
});

describe.skipIf(!contractEnabled)("cycle date-check contract", () => {
  it("answers free, then colliding, then free again once excluded", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-1),
      end_date: instant(1),
    });
    try {
      const free = await checkCycleDates(client, CONTRACT_WORKSPACE, projectId, {
        start_date: calendarDate(10),
        end_date: calendarDate(12),
      });
      expect(free).toEqual({ status: true });

      const collision = await checkCycleDates(client, CONTRACT_WORKSPACE, projectId, {
        start_date: calendarDate(-1),
        end_date: calendarDate(1),
      });
      expect(collision.status).toBe(false);
      expect(collision.error).toContain("cycle already");

      const excluded = await checkCycleDates(client, CONTRACT_WORKSPACE, projectId, {
        start_date: calendarDate(-1),
        end_date: calendarDate(1),
        cycle_id: created.id,
      });
      expect(excluded.status).toBe(true);
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("refuses a check without dates", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await expect(
      checkCycleDates(client, CONTRACT_WORKSPACE, projectId, { start_date: "", end_date: "" })
    ).rejects.toMatchObject({ status: 400 });
  });
});

describe.skipIf(!contractEnabled)("cycle-issues contract", () => {
  it("reads the cycle's items through the hosted-layout shape", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-1),
      end_date: instant(1),
    });
    const issue = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_ISSUE });
    try {
      await addIssueToCycle(client, CONTRACT_WORKSPACE, projectId, created.id, issue.id);
      const page = await listCycleIssues(client, CONTRACT_WORKSPACE, projectId, created.id);
      const row = page.results.find((candidate) => candidate.id === issue.id);
      expect(row).toBeDefined();
      expect(row?.cycle_id).toBe(created.id);
      expect(page.count).toBeGreaterThanOrEqual(1);
    } finally {
      try {
        await removeIssueFromCycle(client, CONTRACT_WORKSPACE, projectId, created.id, issue.id);
      } catch {
        // The membership is gone with the cycle in the common path.
      }
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, issue.id);
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("answers an empty page for a cycle with no items", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-1),
      end_date: instant(1),
    });
    try {
      const page = await listCycleIssues(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(page.results).toEqual([]);
      expect(page.total_results).toBe(0);
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });
});

describe.skipIf(!contractEnabled)("cycle progress and analytics contract", () => {
  it("reads progress counts and both analytics measures", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-1),
      end_date: instant(1),
    });
    try {
      const progress = await getCycleProgress(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(progress.total_issues).toBe(0);
      expect(progress.total_estimate_points === null || typeof progress.total_estimate_points === "number").toBe(true);

      const issues = await getCycleAnalytics(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(issues.assignees).toEqual([]);
      expect(issues.labels).toEqual([]);
      expect(issues.completion_chart).toEqual(expect.any(Object));

      const points = await getCycleAnalytics(client, CONTRACT_WORKSPACE, projectId, created.id, "points");
      expect(points.completion_chart).toEqual(expect.any(Object));
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("404s the progress of a missing cycle", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await expect(
      getCycleProgress(client, CONTRACT_WORKSPACE, projectId, "123e4567-e89b-12d3-a456-4266141749ff")
    ).rejects.toMatchObject({ status: 404 });
  });
});

describe.skipIf(!contractEnabled)("cycle favourites, archive, restore and transfer contract", () => {
  it("favourites and unfavourites through the project endpoints", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, { name: PROBE });
    try {
      await addCycleFavorite(client, CONTRACT_WORKSPACE, projectId, created.id);
      const withFavorite = await listCycleRows(client, CONTRACT_WORKSPACE, projectId);
      expect(withFavorite.find((row) => row.id === created.id)?.is_favorite).toBe(true);
      await removeCycleFavorite(client, CONTRACT_WORKSPACE, projectId, created.id);
      const withoutFavorite = await listCycleRows(client, CONTRACT_WORKSPACE, projectId);
      expect(withoutFavorite.find((row) => row.id === created.id)?.is_favorite).toBe(false);
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("archives a finished cycle, reads it in the archived list and detail, then restores it", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-14),
      end_date: instant(-7),
    });
    let archived = false;
    try {
      expect(created.status).toBe("COMPLETED");
      const result = await archiveCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(typeof result.archived_at).toBe("string");
      archived = true;

      const archivedRows = await listArchivedCycles(client, CONTRACT_WORKSPACE, projectId);
      expect(archivedRows.map((row) => row.id)).toContain(created.id);
      const liveRows = await listCycleRows(client, CONTRACT_WORKSPACE, projectId);
      expect(liveRows.map((row) => row.id)).not.toContain(created.id);

      const detail = await getArchivedCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(detail.id).toBe(created.id);
      expect(detail.archived_at).toBeTruthy();
      expect(detail.distribution?.assignees).toEqual([]);

      await restoreCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
      archived = false;
      const restored = await getCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(restored.id).toBe(created.id);
      const afterRestore = await listArchivedCycles(client, CONTRACT_WORKSPACE, projectId);
      expect(afterRestore.map((row) => row.id)).not.toContain(created.id);
    } finally {
      if (archived) {
        await restoreCycle(client, CONTRACT_WORKSPACE, projectId, created.id).catch(() => undefined);
      }
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("refuses to archive a cycle that has not finished", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, {
      name: PROBE,
      start_date: instant(-1),
      end_date: instant(1),
    });
    try {
      await expect(archiveCycle(client, CONTRACT_WORKSPACE, projectId, created.id)).rejects.toMatchObject({
        status: 400,
      });
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("transfers a probe item into a second cycle and back", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const source = await createCycle(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} source` });
    const target = await createCycle(client, CONTRACT_WORKSPACE, projectId, { name: `${PROBE} target` });
    const issue = await createIssue(client, CONTRACT_WORKSPACE, projectId, { name: PROBE_ISSUE });
    try {
      await addIssueToCycle(client, CONTRACT_WORKSPACE, projectId, source.id, issue.id);
      const moved = await transferCycleIssues(client, CONTRACT_WORKSPACE, projectId, source.id, target.id);
      expect(moved.message).toContain("Success");

      const targetPage = await listCycleIssues(client, CONTRACT_WORKSPACE, projectId, target.id);
      expect(targetPage.results.map((row) => row.id)).toContain(issue.id);
      const sourcePage = await listCycleIssues(client, CONTRACT_WORKSPACE, projectId, source.id);
      expect(sourcePage.results.map((row) => row.id)).not.toContain(issue.id);
    } finally {
      try {
        await removeIssueFromCycle(client, CONTRACT_WORKSPACE, projectId, target.id, issue.id);
      } catch {
        // Already detached by a failed transfer path.
      }
      await deleteIssue(client, CONTRACT_WORKSPACE, projectId, issue.id);
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, source.id);
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, target.id);
    }
  });

  it("refuses a transfer without a destination", async () => {
    const client = await sharedSignedIn();
    const projectId = await seededProjectId(client);
    await clearProbeCycle(client, projectId);
    const created = await createCycle(client, CONTRACT_WORKSPACE, projectId, { name: PROBE });
    try {
      await expect(transferCycleIssues(client, CONTRACT_WORKSPACE, projectId, created.id, "")).rejects.toMatchObject({
        status: 400,
      });
    } finally {
      await deleteCycle(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });
});
