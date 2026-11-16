// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: sidebar preferences and the favorites tree against live Django.
import { describe, expect, it } from "vitest";
import {
  createFavorite,
  deleteFavorite,
  getSidebarPreferences,
  listFavoriteGroup,
  listFavorites,
  updateFavorite,
  updateSidebarPreferences,
} from "../contracts/sidebar.js";
import { listProjects } from "../contracts/projects.js";
import { listIssues } from "../contracts/issues.js";
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

describe.skipIf(!contractEnabled)("sidebar-preferences contract", () => {
  it("answers every known row with pinned state and ordering", async () => {
    const prefs = await getSidebarPreferences(await signedIn(), CONTRACT_WORKSPACE);
    for (const key of ["views", "active_cycles", "analytics", "drafts", "your_work", "archives", "stickies"]) {
      expect(prefs[key]).toBeDefined();
      expect(typeof prefs[key]?.is_pinned).toBe("boolean");
      expect(typeof prefs[key]?.sort_order).toBe("number");
    }
    expect(prefs["drafts"]?.is_pinned).toBe(true);
    expect(prefs["your_work"]?.is_pinned).toBe(true);
    expect(prefs["stickies"]?.is_pinned).toBe(true);
  });

  it("patches pinned rows and restores them", async () => {
    const client = await signedIn();
    const before = await getSidebarPreferences(client, CONTRACT_WORKSPACE);
    const key = "analytics";
    const wasPinned = before[key]?.is_pinned ?? false;
    const result = await updateSidebarPreferences(client, CONTRACT_WORKSPACE, [{ key, is_pinned: !wasPinned }]);
    expect(result.message).toBeDefined();
    const during = await getSidebarPreferences(client, CONTRACT_WORKSPACE);
    expect(during[key]?.is_pinned).toBe(!wasPinned);
    await updateSidebarPreferences(client, CONTRACT_WORKSPACE, [{ key, is_pinned: wasPinned }]);
    const after = await getSidebarPreferences(client, CONTRACT_WORKSPACE);
    expect(after[key]?.is_pinned).toBe(wasPinned);
  });
});

describe.skipIf(!contractEnabled)("favorites contract", () => {
  it("creates folders and entries, groups children, moves, reorders and deletes", async () => {
    const client = await signedIn();
    const projectId = await seededProjectId(client);
    const page = await listIssues(client, CONTRACT_WORKSPACE, projectId);
    const issueId = page.results[0]?.id;
    expect(issueId).toBeDefined();
    if (!issueId) return;

    const folder = await createFavorite(client, CONTRACT_WORKSPACE, {
      entity_type: "folder",
      name: "Contract folder",
      is_folder: true,
    });
    expect(folder.is_folder).toBe(true);
    expect(folder.name).toBe("Contract folder");

    const entry = await createFavorite(client, CONTRACT_WORKSPACE, {
      entity_type: "issue",
      entity_identifier: issueId,
      name: "Contract favorite issue",
      parent: folder.id,
      project_id: projectId,
    });
    expect(entry.parent).toBe(folder.id);

    const children = await listFavoriteGroup(client, CONTRACT_WORKSPACE, folder.id);
    expect(children.map((child) => child.id)).toContain(entry.id);

    const moved = await updateFavorite(client, CONTRACT_WORKSPACE, entry.id, {
      parent: null,
      sequence: 12.5,
    });
    expect(moved.parent).toBeNull();
    expect(moved.sequence).toBe(12.5);

    const top = await listFavorites(client, CONTRACT_WORKSPACE);
    expect(top.map((favorite) => favorite.id)).toContain(entry.id);

    await deleteFavorite(client, CONTRACT_WORKSPACE, entry.id);
    await deleteFavorite(client, CONTRACT_WORKSPACE, folder.id);
    const remaining = await listFavorites(client, CONTRACT_WORKSPACE);
    expect(remaining.map((favorite) => favorite.id)).not.toContain(entry.id);
  });
});
