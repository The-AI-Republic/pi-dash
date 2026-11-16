// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: saved-view create/update thin slice for the issues area
// (ISS-104) against live Django (NEWFRONT-611).
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import {
  createProjectView,
  createWorkspaceView,
  deleteProjectView,
  deleteWorkspaceView,
  getProjectView,
  getWorkspaceView,
  updateProjectView,
  updateWorkspaceView,
} from "../contracts/issue-views.js";
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

const VIEW_PROBE = "Contract view probe";

/** Remove probe views a dead run may have left behind (raw list reads). */
async function clearProbeViews(client: ApiClient, projectId: string): Promise<void> {
  const projectViews = (await client.getJson<unknown>(
    `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/views/`
  )) as { id?: unknown; name?: unknown }[];
  for (const view of projectViews.filter((entry) => entry.name === VIEW_PROBE)) {
    await deleteProjectView(client, CONTRACT_WORKSPACE, projectId, String(view.id));
  }
  const workspaceViews = (await client.getJson<unknown>(`/api/workspaces/${CONTRACT_WORKSPACE}/views/`)) as {
    id?: unknown;
    name?: unknown;
  }[];
  for (const view of workspaceViews.filter((entry) => entry.name === VIEW_PROBE)) {
    await deleteWorkspaceView(client, CONTRACT_WORKSPACE, String(view.id));
  }
}

describe.skipIf(!contractEnabled)("project view contract", () => {
  it("creates, updates and deletes a project view", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbeViews(client, projectId);
    const created = await createProjectView(client, CONTRACT_WORKSPACE, projectId, {
      name: VIEW_PROBE,
      filters: { priority: ["high"] },
      display_filters: { layout: "kanban" },
      display_properties: { state: true },
      rich_filters: {},
    });
    expect(created.name).toBe(VIEW_PROBE);
    expect(created.project).toBe(projectId);
    expect(created.filters).toEqual({ priority: ["high"] });
    expect(created.display_filters).toEqual({ layout: "kanban" });
    const updated = await updateProjectView(client, CONTRACT_WORKSPACE, projectId, created.id, {
      name: `${VIEW_PROBE} edited`,
      filters: { priority: ["urgent"] },
    });
    expect(updated.name).toBe(`${VIEW_PROBE} edited`);
    expect(updated.filters).toEqual({ priority: ["urgent"] });
    const fetched = await getProjectView(client, CONTRACT_WORKSPACE, projectId, created.id);
    expect(fetched.id).toBe(created.id);
    expect(fetched.filters).toEqual({ priority: ["urgent"] });
    await deleteProjectView(client, CONTRACT_WORKSPACE, projectId, created.id);
    // Delete blanks the row in place (retrieve still 200s), so
    // deletion is proven by exclusion from the list.
    const remaining = (await client.getJson<unknown>(
      `/api/workspaces/${CONTRACT_WORKSPACE}/projects/${projectId}/views/`
    )) as { id?: unknown }[];
    expect(remaining.map((view) => view.id)).not.toContain(created.id);
  });
});

describe.skipIf(!contractEnabled)("workspace view contract", () => {
  it("creates, updates and deletes a workspace view", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    await clearProbeViews(client, projectId);
    const created = await createWorkspaceView(client, CONTRACT_WORKSPACE, {
      name: VIEW_PROBE,
      filters: { state_group: ["started"] },
      display_filters: { layout: "list" },
      rich_filters: {},
    });
    expect(created.name).toBe(VIEW_PROBE);
    expect(created.project).toBeNull();
    expect(created.filters).toEqual({ state_group: ["started"] });
    const updated = await updateWorkspaceView(client, CONTRACT_WORKSPACE, created.id, {
      display_filters: { layout: "calendar" },
    });
    expect(updated.display_filters).toEqual({ layout: "calendar" });
    const fetched = await getWorkspaceView(client, CONTRACT_WORKSPACE, created.id);
    expect(fetched.id).toBe(created.id);
    expect(fetched.project).toBeNull();
    await deleteWorkspaceView(client, CONTRACT_WORKSPACE, created.id);
    const remaining = (await client.getJson<unknown>(`/api/workspaces/${CONTRACT_WORKSPACE}/views/`)) as {
      id?: unknown;
    }[];
    expect(remaining.map((view) => view.id)).not.toContain(created.id);
  });
});
