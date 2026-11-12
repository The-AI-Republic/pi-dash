// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: slug checks, workspace creation and membership reads against
// live Django.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import { getMyWorkspaceMembership, listWorkspaces } from "../contracts/workspaces.js";
import { checkWorkspaceSlug, createWorkspace, listWorkspaceMembers } from "../contracts/workspace-management.js";
import { getMyProjectRoles, listProjects } from "../contracts/projects.js";
import { getMe } from "../contracts/users.js";
import { CONTRACT_PROJECT_IDENTIFIER, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

/** Fresh slug per run: the backend reserves slugs of deleted rows. */
function lifecycleSlug(): string {
  const suffix = `${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
  return `ct-ws-${suffix}`;
}

describe.skipIf(!contractEnabled)("workspace slug contract", () => {
  it("reports free, taken and restricted slugs", async () => {
    const client = await signedIn();
    await expect(checkWorkspaceSlug(client, `ct-never-created-${Date.now().toString(36)}`)).resolves.toEqual({
      status: true,
    });
    await expect(checkWorkspaceSlug(client, CONTRACT_WORKSPACE)).resolves.toEqual({ status: false });
    await expect(checkWorkspaceSlug(client, "api")).resolves.toEqual({ status: false });
  });
});

describe.skipIf(!contractEnabled)("workspace creation contract", () => {
  it("creates a workspace and reads it back in the list", async () => {
    const client = await signedIn();
    const me = await getMe(client);
    const slug = lifecycleSlug();
    const created = await createWorkspace(client, {
      name: `Contract ${slug}`,
      slug,
      organization_size: "2-10",
    });
    try {
      expect(created.slug).toBe(slug);
      expect(created.organization_size).toBe("2-10");
      expect(created.owner).toBe(me.id);
      expect(created.role).toBe(20);
      expect(created.total_members).toBe(1);

      await expect(checkWorkspaceSlug(client, slug)).resolves.toEqual({ status: false });
      const workspaces = await listWorkspaces(client);
      expect(workspaces.some((workspace) => workspace.slug === slug)).toBe(true);
    } finally {
      await client.delete(`/api/workspaces/${slug}/`).catch(() => null);
    }
  });

  it("refuses a duplicate slug with a field error", async () => {
    const client = await signedIn();
    const error = await createWorkspace(client, { name: "Dupe", slug: CONTRACT_WORKSPACE }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
    expect((error as ApiError).fields?.["slug"]?.[0]).toMatch(/already exists/);
  });
});

describe.skipIf(!contractEnabled)("workspace membership contract", () => {
  it("reads my membership with role and draft count", async () => {
    const client = await signedIn();
    const me = await getMe(client);
    const membership = await getMyWorkspaceMembership(client, CONTRACT_WORKSPACE);
    expect(membership?.member).toBe(me.id);
    expect(membership?.role).toBe(20);
    expect(membership?.is_active).toBe(true);
    expect(typeof membership?.draft_issue_count).toBe("number");
  });

  it("reports an unknown workspace as no membership", async () => {
    const client = await signedIn();
    await expect(getMyWorkspaceMembership(client, `ct-unknown-${Date.now().toString(36)}`)).resolves.toBeNull();
  });

  it("reads my per-project roles", async () => {
    const client = await signedIn();
    const projects = await listProjects(client, CONTRACT_WORKSPACE);
    const seeded = projects.find((project) => project.identifier === CONTRACT_PROJECT_IDENTIFIER);
    expect(seeded).toBeDefined();
    const roles = await getMyProjectRoles(client, CONTRACT_WORKSPACE);
    expect(roles[seeded?.id ?? ""]).toBe(20);
  });
});

describe.skipIf(!contractEnabled)("workspace members contract", () => {
  it("lists active memberships with the nested member projection", async () => {
    const client = await signedIn();
    const me = await getMe(client);
    const members = await listWorkspaceMembers(client, CONTRACT_WORKSPACE);
    expect(members.length).toBeGreaterThan(0);
    const mine = members.find((membership) => membership.member.id === me.id);
    expect(mine).toBeDefined();
    expect(mine?.role).toBe(20);
    expect(mine?.member.display_name.length).toBeGreaterThan(0);
  });
});
