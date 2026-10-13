// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: projects, states, labels and members against live Django.
import { beforeAll, describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  addFavoriteProject,
  archiveProject,
  createProject,
  deleteProject,
  getProject,
  getProjectUserProperties,
  getWorkspaceUserProperties,
  joinProjects,
  leaveProject,
  listFavoriteProjects,
  listProjectDetails,
  listProjectInvitations,
  listProjects,
  removeFavoriteProject,
  unarchiveProject,
  updateProjectUserProperties,
  updateWorkspaceUserProperties,
} from "../contracts/projects.js";
import { listLabels, listMembers, listStates } from "../contracts/reference.js";
import { signIn } from "../contracts/auth.js";
import {
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_PROJECT_IDENTIFIER,
  CONTRACT_SECONDARY_EMAIL,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
} from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(email: string = CONTRACT_EMAIL): Promise<ApiClient> {
  const client = contractClient();
  const result = await signIn(client, { email, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  return client;
}

async function seededProjectId(client: ApiClient): Promise<string> {
  const projects = await listProjects(client, CONTRACT_WORKSPACE);
  const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
  expect(seeded).toBeDefined();
  return seeded?.id ?? "";
}

// One session per user for the file: the scratch stack throttles anonymous
// hits (CSRF fetch + sign-in POST) to 30/minute per IP, so per-test
// sign-ins would 429 each other when files run together.
let client: ApiClient;
let probe: ApiClient;

beforeAll(async () => {
  client = await signedIn();
  probe = await signedIn(CONTRACT_SECONDARY_EMAIL);
});

describe.skipIf(!contractEnabled)("projects contract", () => {
  it("lists workspace projects", async () => {
    const projects = await listProjects(client, CONTRACT_WORKSPACE);
    expect(projects.some((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER)).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("reference contracts", () => {
  it("lists project states", async () => {
    const states = await listStates(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(states.length).toBeGreaterThan(0);
    expect(states.some((state) => state.default)).toBe(true);
  });

  it("lists project labels", async () => {
    const labels = await listLabels(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(labels.some((label) => label.name === "contract-bug")).toBe(true);
  });

  it("lists project members", async () => {
    const members = await listMembers(client, CONTRACT_WORKSPACE, await seededProjectId(client));
    expect(members.length).toBeGreaterThan(0);
    expect(members.some((member) => member.role === 20)).toBe(true);
  });
});

/**
 * Fresh name/identifier per run: project delete is a soft delete and the
 * backend reserves identifiers (and names) of deleted rows, so reusing a
 * fixed identifier would collide with earlier runs' leftovers.
 */
function lifecycleProject(): { name: string; identifier: string } {
  const suffix = Date.now().toString(36).toUpperCase().slice(-6);
  return { name: `Contract Lifecycle ${suffix}`, identifier: `CTLC${suffix}` };
}

async function cleanupLifecycleProject(client: ApiClient, identifier: string): Promise<void> {
  const details = await listProjectDetails(client, CONTRACT_WORKSPACE);
  const leftover = details.find((project) => project.identifier === identifier);
  if (leftover) await deleteProject(client, CONTRACT_WORKSPACE, leftover.id).catch(() => 0);
}

describe.skipIf(!contractEnabled)("project detail contract", () => {
  it("reads one project row", async () => {
    const id = await seededProjectId(client);
    const row = await getProject(client, CONTRACT_WORKSPACE, id);
    expect(row.id).toBe(id);
    expect(row.identifier).toBe(CONTRACT_PROJECT_IDENTIFIER);
    expect(row.member_role).toBe(20);
  });

  it("reports unknown projects as not-found", async () => {
    const failure = await getProject(client, CONTRACT_WORKSPACE, "00000000-0000-0000-0000-000000000000").catch(
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).status).toBe(404);
    expect((failure as ApiError).message).toBe("Project does not exist");
  });

  it("reports non-member projects as conflict, secret ones as forbidden", async () => {
    // The probe is a workspace member but joins no new project: a public
    // project answers 409, a secret one 403. Fresh identifiers per run for
    // the same soft-delete reason as the lifecycle suite.
    const open = lifecycleProject();
    const secret = lifecycleProject();
    await cleanupLifecycleProject(client, open.identifier);
    await cleanupLifecycleProject(client, secret.identifier);
    try {
      const created = await createProject(client, CONTRACT_WORKSPACE, open);
      const hidden = await createProject(client, CONTRACT_WORKSPACE, { ...secret, network: 0 });
      const denied = await getProject(probe, CONTRACT_WORKSPACE, created.id).catch((error: unknown) => error);
      expect(denied).toBeInstanceOf(ApiError);
      expect((denied as ApiError).status).toBe(409);
      const forbidden = await getProject(probe, CONTRACT_WORKSPACE, hidden.id).catch((error: unknown) => error);
      expect(forbidden).toBeInstanceOf(ApiError);
      expect((forbidden as ApiError).status).toBe(403);
    } finally {
      await cleanupLifecycleProject(client, open.identifier);
      await cleanupLifecycleProject(client, secret.identifier);
    }
  });
});

describe.skipIf(!contractEnabled)("project details contract", () => {
  it("lists rich project rows with membership annotations", async () => {
    const details = await listProjectDetails(client, CONTRACT_WORKSPACE);
    const seeded = details.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
    expect(seeded).toBeDefined();
    expect(typeof seeded?.is_favorite).toBe("boolean");
    expect(Array.isArray(seeded?.members)).toBe(true);
    expect(seeded?.archived_at).toBeNull();
  });
});

describe.skipIf(!contractEnabled)("project lifecycle contract", () => {
  it("creates, archives, unarchives and deletes a project", async () => {
    const input = lifecycleProject();
    await cleanupLifecycleProject(client, input.identifier);

    const created = await createProject(client, CONTRACT_WORKSPACE, input);
    expect(created.identifier).toBe(input.identifier);
    expect(created.archived_at).toBeNull();

    const archived = await archiveProject(client, CONTRACT_WORKSPACE, created.id);
    expect(typeof archived.archived_at).toBe("string");

    await expect(unarchiveProject(client, CONTRACT_WORKSPACE, created.id)).resolves.toBe(204);
    await expect(deleteProject(client, CONTRACT_WORKSPACE, created.id)).resolves.toBe(204);

    const afterDelete = await listProjectDetails(client, CONTRACT_WORKSPACE);
    expect(afterDelete.some((project) => project.identifier === input.identifier)).toBe(false);
  });

  it("refuses the only admin's leave with a 400", async () => {
    const input = lifecycleProject();
    await cleanupLifecycleProject(client, input.identifier);
    const created = await createProject(client, CONTRACT_WORKSPACE, input);
    try {
      const error = await leaveProject(client, CONTRACT_WORKSPACE, created.id).then(
        () => null,
        (caught: unknown) => caught
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(400);
    } finally {
      await deleteProject(client, CONTRACT_WORKSPACE, created.id).catch(() => 0);
    }
  });

  it("lets a second admin leave and rejoin", async () => {
    const projectId = await seededProjectId(probe);
    // Join reactivates the seeded membership, so the suite re-runs clean.
    const joined = await joinProjects(probe, CONTRACT_WORKSPACE, [projectId]);
    expect(joined.message).toBe("Projects joined successfully");
    await expect(leaveProject(probe, CONTRACT_WORKSPACE, projectId)).resolves.toBe(204);
    await expect(joinProjects(probe, CONTRACT_WORKSPACE, [projectId])).resolves.toEqual({
      message: "Projects joined successfully",
    });
  });
});

describe.skipIf(!contractEnabled)("project invitations contract", () => {
  it("lists my invitations and joins by project ids", async () => {
    const invitations = await listProjectInvitations(client, CONTRACT_WORKSPACE);
    expect(Array.isArray(invitations)).toBe(true);
    const projectId = await seededProjectId(client);
    await expect(joinProjects(client, CONTRACT_WORKSPACE, [projectId])).resolves.toEqual({
      message: "Projects joined successfully",
    });
  });
});

describe.skipIf(!contractEnabled)("favorite projects contract", () => {
  it("adds, lists and removes a favorite", async () => {
    const projectId = await seededProjectId(client);
    // Drop a leftover from a crashed run so create stays single-row.
    await removeFavoriteProject(client, CONTRACT_WORKSPACE, projectId).catch(() => 0);

    await expect(addFavoriteProject(client, CONTRACT_WORKSPACE, projectId)).resolves.toBe(204);
    const listed = await listFavoriteProjects(client, CONTRACT_WORKSPACE);
    expect(listed.some((favorite) => favorite.entity_identifier === projectId)).toBe(true);

    await expect(removeFavoriteProject(client, CONTRACT_WORKSPACE, projectId)).resolves.toBe(204);
    const afterRemove = await listFavoriteProjects(client, CONTRACT_WORKSPACE);
    expect(afterRemove.some((favorite) => favorite.entity_identifier === projectId)).toBe(false);
  });
});

describe.skipIf(!contractEnabled)("user properties contracts", () => {
  it("reads and patches workspace user properties", async () => {
    const before = await getWorkspaceUserProperties(client, CONTRACT_WORKSPACE);
    const nextLimit = before.navigation_project_limit === 10 ? 5 : 10;
    const updated = await updateWorkspaceUserProperties(client, CONTRACT_WORKSPACE, {
      navigation_project_limit: nextLimit,
    });
    expect(updated.navigation_project_limit).toBe(nextLimit);
    const restored = await updateWorkspaceUserProperties(client, CONTRACT_WORKSPACE, {
      navigation_project_limit: before.navigation_project_limit,
    });
    expect(restored.navigation_project_limit).toBe(before.navigation_project_limit);
  });

  it("reads and patches project user properties", async () => {
    const projectId = await seededProjectId(client);
    const before = await getProjectUserProperties(client, CONTRACT_WORKSPACE, projectId);
    const updated = await updateProjectUserProperties(client, CONTRACT_WORKSPACE, projectId, {
      sort_order: before.sort_order + 1,
    });
    expect(updated.sort_order).toBe(before.sort_order + 1);
    const restored = await updateProjectUserProperties(client, CONTRACT_WORKSPACE, projectId, {
      sort_order: before.sort_order,
    });
    expect(restored.sort_order).toBe(before.sort_order);
  });
});
