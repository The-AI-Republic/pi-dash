// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: project deploy boards against live Django. The board is
// publish state for one project, so the suite publishes on a throwaway
// project (the seeded project may already carry a board another suite
// relies on), round-trips every control, unpublishes, and reads the
// all-null shell back.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  createProjectDeployBoard,
  deleteProjectDeployBoard,
  getProjectDeployBoard,
  updateProjectDeployBoard,
} from "../contracts/project-deploy-boards.js";
import { createProject, deleteProject } from "../contracts/project-management.js";
import { CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

function identifier(prefix: string): string {
  return `${prefix}${Math.random().toString(36).slice(2, 7)}`.slice(0, 10).toUpperCase();
}

describe.skipIf(!contractEnabled)("project deploy board contract", () => {
  it("publishes, updates, reads and unpublishes a project board", async () => {
    const client = await signedIn();
    const project = await createProject(client, CONTRACT_WORKSPACE, {
      name: `Contract board ${Date.now().toString(36)}`,
      identifier: identifier("CDB"),
    });
    try {
      const empty = await getProjectDeployBoard(client, CONTRACT_WORKSPACE, project.id);
      expect(empty).not.toHaveProperty("anchor");
      expect(empty.entity_identifier).toBeNull();

      const published = await createProjectDeployBoard(client, CONTRACT_WORKSPACE, project.id, {
        views: { list: true, kanban: true },
        is_comments_enabled: true,
        is_reactions_enabled: false,
        is_votes_enabled: false,
      });
      expect(published.project).toBe(project.id);
      expect(published.entity_name).toBe("project");
      expect(published.anchor?.length).toBeGreaterThan(0);
      expect(published.view_props).toEqual({ list: true, kanban: true });
      expect(published.is_comments_enabled).toBe(true);

      const read = await getProjectDeployBoard(client, CONTRACT_WORKSPACE, project.id);
      expect(read).toMatchObject({ id: published.id, anchor: published.anchor });

      const updated = await updateProjectDeployBoard(client, CONTRACT_WORKSPACE, project.id, published.id, {
        view_props: { list: true },
        is_votes_enabled: true,
      });
      expect(updated.view_props).toEqual({ list: true });
      expect(updated.is_votes_enabled).toBe(true);
      expect(updated.anchor).toBe(published.anchor);

      await expect(deleteProjectDeployBoard(client, CONTRACT_WORKSPACE, project.id, published.id)).resolves.toBe(204);
      const after = await getProjectDeployBoard(client, CONTRACT_WORKSPACE, project.id);
      expect(after).not.toHaveProperty("anchor");
      expect(after.entity_name).toBe("");
    } finally {
      await deleteProject(client, CONTRACT_WORKSPACE, project.id).catch(() => 0);
    }
  });

  it("404s an update for a board that does not exist", async () => {
    const client = await signedIn();
    const project = await createProject(client, CONTRACT_WORKSPACE, {
      name: `Contract board miss ${Date.now().toString(36)}`,
      identifier: identifier("CDX"),
    });
    try {
      const missed = await updateProjectDeployBoard(
        client,
        CONTRACT_WORKSPACE,
        project.id,
        "00000000-0000-4000-8000-000000000000",
        { is_votes_enabled: true }
      ).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(missed).toBeInstanceOf(ApiError);
      expect((missed as ApiError).status).toBe(404);
    } finally {
      await deleteProject(client, CONTRACT_WORKSPACE, project.id).catch(() => 0);
    }
  });
});
