// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: cycle and module user-properties GET/PATCH against live
// Django (NEWFRONT-611). Epics user-properties are cloud-only (no OSS
// route) and are proven by stub unit tests in contracts.test.ts instead.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { listCycles } from "../contracts/cycles.js";
import {
  getCycleUserProperties,
  getModuleUserProperties,
  updateCycleUserProperties,
  updateModuleUserProperties,
} from "../contracts/issue-prefs.js";
import { listModules } from "../contracts/modules.js";
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

describe.skipIf(!contractEnabled)("cycle user-properties contract", () => {
  it("reads defaults, flips the layout, then restores", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const cycles = await listCycles(client, CONTRACT_WORKSPACE, projectId);
    const cycle = cycles.find((row) => row.name === "Contract cycle");
    expect(cycle).toBeDefined();
    if (!cycle) return;
    const before = await getCycleUserProperties(client, CONTRACT_WORKSPACE, projectId, cycle.id);
    expect(before.cycle).toBe(cycle.id);
    // Flip away from whatever a dead run may have left behind, then
    // restore — the suite self-heals instead of pinning "list".
    const next = before.display_filters["layout"] === "kanban" ? "list" : "kanban";
    try {
      const flipped = await updateCycleUserProperties(client, CONTRACT_WORKSPACE, projectId, cycle.id, {
        display_filters: { ...before.display_filters, layout: next },
      });
      expect(flipped.display_filters["layout"]).toBe(next);
      // PATCH replaces the sent key but leaves the others alone.
      expect(flipped.display_properties).toEqual(before.display_properties);
      const reread = await getCycleUserProperties(client, CONTRACT_WORKSPACE, projectId, cycle.id);
      expect(reread.display_filters["layout"]).toBe(next);
    } finally {
      await updateCycleUserProperties(client, CONTRACT_WORKSPACE, projectId, cycle.id, {
        display_filters: before.display_filters,
      });
    }
    const restored = await getCycleUserProperties(client, CONTRACT_WORKSPACE, projectId, cycle.id);
    expect(restored.display_filters).toEqual(before.display_filters);
  });
});

describe.skipIf(!contractEnabled)("module user-properties contract", () => {
  it("reads defaults, flips the layout, then restores", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const modules = await listModules(client, CONTRACT_WORKSPACE, projectId);
    const module = modules.find((row) => row.name === "Contract module");
    expect(module).toBeDefined();
    if (!module) return;
    const before = await getModuleUserProperties(client, CONTRACT_WORKSPACE, projectId, module.id);
    expect(before.module).toBe(module.id);
    const next = before.display_filters["layout"] === "calendar" ? "list" : "calendar";
    try {
      const flipped = await updateModuleUserProperties(client, CONTRACT_WORKSPACE, projectId, module.id, {
        display_filters: { ...before.display_filters, layout: next },
      });
      expect(flipped.display_filters["layout"]).toBe(next);
      expect(flipped.display_properties).toEqual(before.display_properties);
      const reread = await getModuleUserProperties(client, CONTRACT_WORKSPACE, projectId, module.id);
      expect(reread.display_filters["layout"]).toBe(next);
    } finally {
      await updateModuleUserProperties(client, CONTRACT_WORKSPACE, projectId, module.id, {
        display_filters: before.display_filters,
      });
    }
    const restored = await getModuleUserProperties(client, CONTRACT_WORKSPACE, projectId, module.id);
    expect(restored.display_filters).toEqual(before.display_filters);
  });
});
