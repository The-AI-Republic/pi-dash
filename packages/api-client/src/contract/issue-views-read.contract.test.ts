// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: the views area's reads and writes against live Django
// (parity/specs/views.md §2, NEWFRONT-718) — both scopes' lists and
// retrieves, the blank-record stand-in for a missing id, the patch echo
// for every dialog-written field, guest narrowing, the generic favorites
// round trip behind a view star, and the issue lists the view bodies
// mount.
import { describe, expect, it } from "vitest";
import type { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { signIn } from "../contracts/auth-entry.js";
import {
  addViewFavorite,
  createProjectView,
  createWorkspaceView,
  deleteProjectView,
  deleteWorkspaceView,
  getProjectView,
  getWorkspaceView,
  listProjectViews,
  listWorkspaceViews,
  removeViewFavorite,
  updateProjectView,
  updateWorkspaceView,
} from "../contracts/issue-views.js";
import { listGlobalIssues, listIssues } from "../contracts/issues.js";
import { listProjects } from "../contracts/projects.js";
import { deleteFavorite, listFavorites } from "../contracts/sidebar.js";
import { getMe } from "../contracts/users.js";
import {
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";

const GUEST_EMAIL = process.env["PIDASH_CONTRACT_GUEST_EMAIL"] ?? "contract.guest@example.com";
const MISSING_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_PROBE = "Views probe: project";
const WORKSPACE_PROBE = "Views probe: workspace";
const GUEST_PROBE = "Views probe: guest-owned";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

async function seededProject(client: ApiClient) {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded;
}

async function seededProjectId(client: ApiClient): Promise<string> {
  return (await seededProject(client))?.id ?? "";
}

/** A second session as the seeded guest, for the narrowing assertions. */
async function guestClient(): Promise<ApiClient> {
  const client = contractClient();
  const result = await signIn(client, { email: GUEST_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  return client;
}

describe.skipIf(!contractEnabled)("project view list and retrieve", () => {
  it("lists the caller's project views and reads one back", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await createProjectView(client, CONTRACT_WORKSPACE, projectId, {
      name: PROJECT_PROBE,
      description: "probe",
      filters: { priority: ["high"] },
      display_filters: { layout: "list" },
      display_properties: { priority: true },
      rich_filters: { priority__in: "high" },
      logo_props: { in_use: true },
    });
    try {
      const list = await listProjectViews(client, CONTRACT_WORKSPACE, projectId);
      const row = list.find((view) => view.id === created.id);
      expect(row).toBeDefined();
      expect(row?.project).toBe(projectId);
      expect(row?.is_favorite).toBe(false);
      const fetched = await getProjectView(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(fetched.id).toBe(created.id);
      expect(fetched.name).toBe(PROJECT_PROBE);
    } finally {
      await deleteProjectView(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("reports an unknown project view id as a 404 instead of a blank record", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const failure = await getProjectView(client, CONTRACT_WORKSPACE, projectId, MISSING_ID).catch(
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
  });

  it("echoes every field the edit dialog writes", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const created = await createProjectView(client, CONTRACT_WORKSPACE, projectId, { name: PROJECT_PROBE });
    try {
      // access/is_locked are serializer read-only: posting them proves the
      // server keeps its own values rather than the caller's.
      const body = {
        name: `${PROJECT_PROBE} edited`,
        description: "edited description",
        filters: { priority: ["urgent"] },
        display_filters: { layout: "kanban", group_by: "state" },
        display_properties: { priority: true, state: true },
        rich_filters: { and: [{ priority__in: "urgent" }] },
        logo_props: { in_use: true, emoji: "x" },
        access: 0,
        is_locked: true,
      };
      const updated = await updateProjectView(client, CONTRACT_WORKSPACE, projectId, created.id, body);
      expect(updated.name).toBe(body.name);
      expect(updated.description).toBe(body.description);
      expect(updated.filters).toEqual(body.filters);
      // The server recomputes `query` from the filter expression.
      expect(updated.query).toEqual({ priority__in: ["urgent"] });
      expect(updated.display_filters).toEqual(body.display_filters);
      expect(updated.display_properties).toEqual(body.display_properties);
      expect(updated.rich_filters).toEqual(body.rich_filters);
      expect(updated.logo_props).toEqual(body.logo_props);
      expect(updated.access).toBe(1);
      expect(updated.is_locked).toBe(false);
      const reread = await getProjectView(client, CONTRACT_WORKSPACE, projectId, created.id);
      expect(reread).toMatchObject({
        name: body.name,
        display_filters: body.display_filters,
        rich_filters: body.rich_filters,
        logo_props: body.logo_props,
      });
    } finally {
      await deleteProjectView(client, CONTRACT_WORKSPACE, projectId, created.id);
    }
  });

  it("narrows a guest without the project flag to its own project views", async () => {
    const member = await signedIn();
    const project = await seededProject(member);
    const projectId = project?.id ?? "";
    expect(project?.guest_view_all_features).toBe(false);
    const shared = await createProjectView(member, CONTRACT_WORKSPACE, projectId, { name: PROJECT_PROBE });
    const guest = await guestClient();
    try {
      const memberList = await listProjectViews(member, CONTRACT_WORKSPACE, projectId);
      expect(memberList.some((view) => view.id === shared.id)).toBe(true);

      const guestList = await listProjectViews(guest, CONTRACT_WORKSPACE, projectId);
      expect(guestList.some((view) => view.id === shared.id)).toBe(false);

      const owned = await createProjectView(guest, CONTRACT_WORKSPACE, projectId, { name: GUEST_PROBE });
      const narrowed = await listProjectViews(guest, CONTRACT_WORKSPACE, projectId);
      expect(narrowed.map((view) => view.id)).toEqual(expect.arrayContaining([owned.id]));
      expect(narrowed.some((view) => view.id === shared.id)).toBe(false);
      // The guest's own row is readable and writable for the guest.
      expect((await getProjectView(guest, CONTRACT_WORKSPACE, projectId, owned.id)).id).toBe(owned.id);
      const updated = await updateProjectView(guest, CONTRACT_WORKSPACE, projectId, owned.id, {
        name: `${GUEST_PROBE} edited`,
      });
      expect(updated.name).toBe(`${GUEST_PROBE} edited`);
      await deleteProjectView(guest, CONTRACT_WORKSPACE, projectId, owned.id);
      // Another member's shared view is not reachable through the guest's
      // narrowed list even by direct id: the read is refused, not served.
      const hidden = await getProjectView(guest, CONTRACT_WORKSPACE, projectId, shared.id).catch(
        (error: unknown) => error
      );
      expect(hidden).toBeInstanceOf(ApiError);
      expect([403, 404, 500]).toContain((hidden as ApiError).status);
    } finally {
      await deleteProjectView(member, CONTRACT_WORKSPACE, projectId, shared.id);
    }
  });
});

describe.skipIf(!contractEnabled)("workspace view list and retrieve", () => {
  it("lists custom workspace views and reads one back", async () => {
    const client = await signedIn();
    const created = await createWorkspaceView(client, CONTRACT_WORKSPACE, {
      name: WORKSPACE_PROBE,
      display_filters: { layout: "spreadsheet", order_by: "-created_at" },
      rich_filters: {},
    });
    try {
      const list = await listWorkspaceViews(client, CONTRACT_WORKSPACE, { orderBy: "-created_at" });
      const row = list.find((view) => view.id === created.id);
      expect(row).toBeDefined();
      expect(row?.project).toBeNull();
      expect(row?.display_filters).toEqual({ layout: "spreadsheet", order_by: "-created_at" });
      const fetched = await getWorkspaceView(client, CONTRACT_WORKSPACE, created.id);
      expect(fetched.id).toBe(created.id);
      expect(fetched.project).toBeNull();
    } finally {
      await deleteWorkspaceView(client, CONTRACT_WORKSPACE, created.id);
    }
  });

  it("reports an unknown workspace view id as a 404", async () => {
    const client = await signedIn();
    await expect(getWorkspaceView(client, CONTRACT_WORKSPACE, MISSING_ID)).rejects.toMatchObject({ status: 404 });
  });

  it("echoes the workspace edit fields", async () => {
    const client = await signedIn();
    const created = await createWorkspaceView(client, CONTRACT_WORKSPACE, { name: WORKSPACE_PROBE });
    try {
      const updated = await updateWorkspaceView(client, CONTRACT_WORKSPACE, created.id, {
        name: `${WORKSPACE_PROBE} edited`,
        description: "edited",
        display_filters: { layout: "calendar", calendar: { mode: "month", weekend: false } },
        rich_filters: { state_group__in: ["started"] },
        logo_props: { in_use: true, emoji: "y" },
      });
      expect(updated.name).toBe(`${WORKSPACE_PROBE} edited`);
      expect(updated.description).toBe("edited");
      expect(updated.display_filters).toEqual({ layout: "calendar", calendar: { mode: "month", weekend: false } });
      expect(updated.rich_filters).toEqual({ state_group__in: ["started"] });
      expect(updated.logo_props).toEqual({ in_use: true, emoji: "y" });
      expect(updated.access).toBe(1);
    } finally {
      await deleteWorkspaceView(client, CONTRACT_WORKSPACE, created.id);
    }
  });

  it("narrows a workspace guest to the customs it owns", async () => {
    const member = await signedIn();
    const shared = await createWorkspaceView(member, CONTRACT_WORKSPACE, { name: WORKSPACE_PROBE });
    const guest = await guestClient();
    try {
      const guestList = await listWorkspaceViews(guest, CONTRACT_WORKSPACE);
      expect(guestList.some((view) => view.id === shared.id)).toBe(false);
      const owned = await createWorkspaceView(guest, CONTRACT_WORKSPACE, { name: GUEST_PROBE });
      const narrowed = await listWorkspaceViews(guest, CONTRACT_WORKSPACE);
      expect(narrowed.map((view) => view.id)).toEqual([owned.id]);
      await deleteWorkspaceView(guest, CONTRACT_WORKSPACE, owned.id);
    } finally {
      await deleteWorkspaceView(member, CONTRACT_WORKSPACE, shared.id);
    }
  });
});

describe.skipIf(!contractEnabled)("view favorites", () => {
  it("stars a view through the generic favorites collection and unstars it", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const view = await createProjectView(client, CONTRACT_WORKSPACE, projectId, { name: PROJECT_PROBE });
    let favoriteId: string | null = null;
    try {
      const favorite = await addViewFavorite(client, CONTRACT_WORKSPACE, view);
      favoriteId = favorite.id;
      expect(favorite.entity_type).toBe("view");
      expect(favorite.entity_identifier).toBe(view.id);
      expect(favorite.project_id).toBe(projectId);

      // Re-adding is idempotent and answers the same row.
      const again = await addViewFavorite(client, CONTRACT_WORKSPACE, view);
      expect(again.id).toBe(favorite.id);

      const favorites = await listFavorites(client, CONTRACT_WORKSPACE);
      expect(favorites.some((entry) => entry.id === favorite.id && entry.entity_type === "view")).toBe(true);
      const list = await listProjectViews(client, CONTRACT_WORKSPACE, projectId);
      expect(list.find((entry) => entry.id === view.id)?.is_favorite).toBe(true);

      await removeViewFavorite(client, CONTRACT_WORKSPACE, favorite.id);
      favoriteId = null;
      const listAfter = await listProjectViews(client, CONTRACT_WORKSPACE, projectId);
      expect(listAfter.find((entry) => entry.id === view.id)?.is_favorite).toBe(false);
      const favoritesAfter = await listFavorites(client, CONTRACT_WORKSPACE);
      expect(favoritesAfter.some((entry) => entry.id === favorite.id)).toBe(false);
    } finally {
      if (favoriteId !== null) await deleteFavorite(client, CONTRACT_WORKSPACE, favoriteId).catch(() => null);
      await deleteProjectView(client, CONTRACT_WORKSPACE, projectId, view.id);
    }
  });
});

describe.skipIf(!contractEnabled)("issue reads behind a view", () => {
  it("applies a stored expression as the list's filter parameters", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const me = await getMe(client);
    const base = await listIssues(client, CONTRACT_WORKSPACE, projectId, { perPage: 100 });
    expect(base.results.length).toBeGreaterThan(0);
    const priorities = [...new Set(base.results.map((row) => row.priority))];
    expect(priorities.length).toBeGreaterThan(0);
    const priority = priorities[0] ?? "medium";

    const filtered = await listIssues(client, CONTRACT_WORKSPACE, projectId, {
      perPage: 100,
      filters: { filters: JSON.stringify({ priority__in: [priority] }) },
    });
    expect(filtered.results.length).toBeGreaterThan(0);
    for (const row of filtered.results) expect(row.priority).toBe(priority);
    expect(filtered.results.length).toBeLessThanOrEqual(base.results.length);

    // Grouping and cursor paging ride the same list read.
    const first = await listIssues(client, CONTRACT_WORKSPACE, projectId, {
      perPage: 1,
      orderBy: "-created_at",
      filters: { filters: JSON.stringify({ priority__in: [priority] }) },
    });
    expect(first.results).toHaveLength(1);
    if (first.next_page_results) {
      const second = await listIssues(client, CONTRACT_WORKSPACE, projectId, {
        perPage: 1,
        orderBy: "-created_at",
        cursor: first.next_cursor,
        filters: { filters: JSON.stringify({ priority__in: [priority] }) },
      });
      expect(second.results[0]?.id).not.toBe(first.results[0]?.id);
    }

    // The workspace default views' static conditions are the bare list
    // parameters the workspace all-issues read accepts.
    const assigned = await listGlobalIssues(client, CONTRACT_WORKSPACE, {
      perPage: 100,
      filters: { assignees: me.id, subscriber: me.id },
    });
    expect(assigned.count).toBeGreaterThanOrEqual(1);
    for (const row of assigned.results) expect(row.assignee_ids).toContain(me.id);
    const created = await listGlobalIssues(client, CONTRACT_WORKSPACE, {
      perPage: 100,
      filters: { created_by: me.id },
    });
    expect(created.count).toBeGreaterThanOrEqual(1);
    for (const row of created.results) expect(row.created_by).toBe(me.id);
  });
});
