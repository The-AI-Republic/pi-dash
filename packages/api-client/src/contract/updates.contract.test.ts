// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: thin work-item, cycle, module and page updates plus issue
// subscription against live Django. Seeded rows come from
// contract/seed-contracts.py; list responses are read raw (no list
// contracts — those belong to their own areas) to resolve handles.
import { describe, expect, it } from "vitest";
import { getIssue, listIssues } from "../contracts/issues.js";
import {
  addIssueToCycle,
  deleteIssue,
  getSubscriptionStatus,
  removeIssueFromCycle,
  setIssueModules,
  subscribeIssue,
  unsubscribeIssue,
  updateIssue,
} from "../contracts/issue-actions.js";
import { listCycles, updateCycle } from "../contracts/cycles.js";
import { listModules, updateModule } from "../contracts/modules.js";
import {
  addPageFavorite,
  archivePage,
  lockPage,
  removePageFavorite,
  restorePage,
  setPageAccess,
  unlockPage,
  updatePage,
} from "../contracts/pages.js";
import { listProjectEstimates } from "../contracts/estimates.js";
import { listProjects } from "../contracts/projects.js";
import { listLabels } from "../contracts/reference.js";
import { getMe } from "../contracts/users.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

/** Resolve a seeded row id from an uncontracted list endpoint by name. */
async function seededRowId(client: ApiClient, path: string, name: string): Promise<string> {
  const body = await client.getJson<unknown>(path);
  const rows = Array.isArray(body) ? body : ((body as { results?: unknown[] }).results ?? []);
  const row = (rows as { id?: unknown; name?: unknown }[]).find((item) => item.name === name);
  expect(row?.id).toBeDefined();
  return String(row?.id ?? "");
}

describe.skipIf(!contractEnabled)("thin issue update contract", () => {
  it("renames the seeded issue and restores it", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    // The update endpoint answers 204 with no body; the rename is proven
    // by re-reading the detail.
    await updateIssue(client, CONTRACT_WORKSPACE, projectId, first.id, { name: "Contract issue renamed" });
    const renamed = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(renamed.name).toBe("Contract issue renamed");
    await updateIssue(client, CONTRACT_WORKSPACE, projectId, first.id, { name: first.name });
    const restored = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(restored.name).toBe(first.name);
  });
});

describe.skipIf(!contractEnabled)("issue subscription contract", () => {
  it("subscribes, reports status and unsubscribes", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    try {
      const before = await getSubscriptionStatus(client, CONTRACT_WORKSPACE, projectId, first.id);
      expect(before.subscribed).toBe(false);
      const subscription = await subscribeIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
      expect(typeof subscription.id).toBe("string");
      const during = await getSubscriptionStatus(client, CONTRACT_WORKSPACE, projectId, first.id);
      expect(during.subscribed).toBe(true);
    } finally {
      await unsubscribeIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    }
    const after = await getSubscriptionStatus(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(after.subscribed).toBe(false);
  });
});

describe.skipIf(!contractEnabled)("thin cycle/module/page update contracts", () => {
  it("renames the seeded cycle and restores it", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const cycleId = await seededRowId(
      client,
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/cycles/`,
      "Contract cycle"
    );
    const renamed = await updateCycle(client, CONTRACT_WORKSPACE, projectId, cycleId, {
      name: "Contract cycle renamed",
    });
    expect(renamed.id).toBe(cycleId);
    expect(renamed.name).toBe("Contract cycle renamed");
    const restored = await updateCycle(client, CONTRACT_WORKSPACE, projectId, cycleId, { name: "Contract cycle" });
    expect(restored.name).toBe("Contract cycle");
  });

  it("renames the seeded module and restores it", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const moduleId = await seededRowId(
      client,
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/modules/`,
      "Contract module"
    );
    const renamed = await updateModule(client, CONTRACT_WORKSPACE, projectId, moduleId, {
      name: "Contract module renamed",
    });
    expect(renamed.id).toBe(moduleId);
    expect(renamed.name).toBe("Contract module renamed");
    const restored = await updateModule(client, CONTRACT_WORKSPACE, projectId, moduleId, {
      name: "Contract module",
    });
    expect(restored.name).toBe("Contract module");
  });

  it("renames the seeded page and restores it", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const pageId = await seededRowId(
      client,
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/pages/`,
      "Contract page"
    );
    const renamed = await updatePage(client, CONTRACT_WORKSPACE, projectId, pageId, {
      name: "Contract page renamed",
    });
    expect(renamed.id).toBe(pageId);
    expect(renamed.name).toBe("Contract page renamed");
    const restored = await updatePage(client, CONTRACT_WORKSPACE, projectId, pageId, { name: "Contract page" });
    expect(restored.name).toBe("Contract page");
  });
});

describe.skipIf(!contractEnabled)("thin contextual field contracts", () => {
  it("patches priority, assignees, labels and estimate, then restores", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    const me = await getMe(client);
    const labels = await listLabels(client, CONTRACT_WORKSPACE, projectId);
    const estimates = await listProjectEstimates(client, CONTRACT_WORKSPACE, projectId);
    const point = estimates.find((estimate) => estimate.name === "Contract estimate")?.points[0];
    expect(labels.length).toBeGreaterThan(0);
    expect(point).toBeDefined();
    const before = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    try {
      await updateIssue(client, CONTRACT_WORKSPACE, projectId, first.id, {
        priority: "high",
        assignee_ids: [me.id],
        label_ids: labels.map((label) => label.id),
        estimate_point: point?.id ?? null,
      });
      const patched = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
      expect(patched.priority).toBe("high");
      expect(patched.assignee_ids).toContain(me.id);
      expect(patched.label_ids).toEqual(labels.map((label) => label.id));
      expect(patched.estimate_point).toBe(point?.id ?? null);
    } finally {
      await updateIssue(client, CONTRACT_WORKSPACE, projectId, first.id, {
        priority: before.priority,
        assignee_ids: before.assignee_ids,
        label_ids: before.label_ids,
        estimate_point: typeof before.estimate_point === "string" ? before.estimate_point : null,
      });
    }
    const restored = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(restored.priority).toBe(before.priority);
    expect(restored.assignee_ids).toEqual(before.assignee_ids);
  });

  it("moves the seeded issue through the seeded cycle and module", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    const first = page.results[0];
    expect(first).toBeDefined();
    if (!first) return;
    const cycles = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const modules = await listModules(client, CONTRACT_WORKSPACE, projectId);
    const cycle = cycles.find((row) => row.name === "Contract cycle");
    const module = modules.find((row) => row.name === "Contract module");
    expect(cycle).toBeDefined();
    expect(module).toBeDefined();
    if (!cycle || !module) return;
    const before = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    try {
      await addIssueToCycle(client, CONTRACT_WORKSPACE, projectId, cycle.id, first.id);
      await setIssueModules(client, CONTRACT_WORKSPACE, projectId, first.id, { modules: [module.id] });
      const moved = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
      expect(moved.cycle_id).toBe(cycle.id);
      expect(moved.module_ids).toContain(module.id);
    } finally {
      await removeIssueFromCycle(client, CONTRACT_WORKSPACE, projectId, cycle.id, first.id);
      await setIssueModules(client, CONTRACT_WORKSPACE, projectId, first.id, {
        modules: [],
        removed_modules: [module.id],
      });
    }
    const restored = await getIssue(client, CONTRACT_WORKSPACE, projectId, first.id);
    expect(restored.cycle_id).toBe(before.cycle_id);
    expect(restored.module_ids).not.toContain(module.id);
  });

  it("deletes a probe issue created raw (no create contract in the shell)", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    // Issue creation belongs to the issues area; the probe is created raw
    // (same precedent as the raw list reads above) and removed by contract.
    const created = await client.sendJson<unknown>(
      "POST",
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/issues/`,
      { json: { name: "Contract delete probe" } }
    );
    const probeId = (created.data as { id?: unknown }).id;
    expect(typeof probeId).toBe("string");
    if (typeof probeId !== "string") return;
    await deleteIssue(client, CONTRACT_WORKSPACE, projectId, probeId);
    await expect(getIssue(client, CONTRACT_WORKSPACE, projectId, probeId)).rejects.toMatchObject({ status: 404 });
  });

  it("patches module members and status, then restores", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const moduleId = await seededRowId(
      client,
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/modules/`,
      "Contract module"
    );
    const body = await client.getJson<unknown>(
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/modules/${moduleId}/`
    );
    const original = body as { status?: unknown; member_ids?: unknown };
    const me = await getMe(client);
    const updated = await updateModule(client, CONTRACT_WORKSPACE, projectId, moduleId, {
      status: "in-progress",
      member_ids: [me.id],
    });
    expect(updated.id).toBe(moduleId);
    const restored = await updateModule(client, CONTRACT_WORKSPACE, projectId, moduleId, {
      status: typeof original.status === "string" ? original.status : "planned",
      member_ids: Array.isArray(original.member_ids) ? (original.member_ids as string[]) : [],
    });
    expect(restored.id).toBe(moduleId);
  });
});

describe.skipIf(!contractEnabled)("thin page action contracts", () => {
  it("locks, toggles access, archives, restores and favorites the seeded page", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const pageId = await seededRowId(
      client,
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/pages/`,
      "Contract page"
    );
    // The seeded page is owned by the contract user, so every capability holds.
    await lockPage(client, CONTRACT_WORKSPACE, projectId, pageId);
    await unlockPage(client, CONTRACT_WORKSPACE, projectId, pageId);
    await setPageAccess(client, CONTRACT_WORKSPACE, projectId, pageId, 1);
    await setPageAccess(client, CONTRACT_WORKSPACE, projectId, pageId, 0);
    await addPageFavorite(client, CONTRACT_WORKSPACE, projectId, pageId);
    await removePageFavorite(client, CONTRACT_WORKSPACE, projectId, pageId);
    const archived = await archivePage(client, CONTRACT_WORKSPACE, projectId, pageId);
    expect(typeof archived.archived_at).toBe("string");
    await restorePage(client, CONTRACT_WORKSPACE, projectId, pageId);
  });
});
