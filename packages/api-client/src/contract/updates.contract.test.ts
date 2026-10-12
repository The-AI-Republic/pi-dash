// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: thin work-item, cycle, module and page updates plus issue
// subscription against live Django. Seeded rows come from
// contract/seed-contracts.py; list responses are read raw (no list
// contracts — those belong to their own areas) to resolve handles.
import { describe, expect, it } from "vitest";
import {
  getIssue,
  getSubscriptionStatus,
  listIssues,
  subscribeIssue,
  unsubscribeIssue,
  updateIssue,
} from "../contracts/issues.js";
import { updateCycle } from "../contracts/cycles.js";
import { updateModule } from "../contracts/modules.js";
import { updatePage } from "../contracts/pages.js";
import { listProjects } from "../contracts/projects.js";
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
