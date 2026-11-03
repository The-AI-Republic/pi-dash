// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: instance-admin workspace list, slug probe and create against
// live Django. The seed user holds an InstanceAdmin grant (see
// contract/seed-contracts.py); the suite signs in through the native admin
// form so the jar carries the admin-session-id cookie the /instances/
// middleware reads.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  checkInstanceWorkspaceSlug,
  createInstanceWorkspace,
  listInstanceWorkspaces,
} from "../contracts/admin-workspaces.js";
import { CONTRACT_EMAIL, CONTRACT_WORKSPACE, contractEnabled, sharedAdminSignedIn, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedAdminSignedIn();
}

/** Fresh slug per run: the backend reserves slugs of deleted rows. */
function lifecycleSlug(prefix: string): string {
  const suffix = `${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;
  return `${prefix}-${suffix}`.toLowerCase();
}

describe.skipIf(!contractEnabled)("instance workspace list contract", () => {
  it("lists workspaces with the seeded row and cursor pagination", async () => {
    const client = await signedIn();
    const page = await listInstanceWorkspaces(client);
    expect(page.total_count).toBeGreaterThan(0);
    expect(page.results.length).toBeGreaterThan(0);
    const seeded = page.results.find((row) => row.slug === CONTRACT_WORKSPACE);
    expect(seeded).toBeDefined();
    expect(seeded?.name.length).toBeGreaterThan(0);
    expect(page.next_cursor.length).toBeGreaterThan(0);
    expect(typeof page.next_page_results).toBe("boolean");
  });

  it("follows the next cursor when more pages exist", async () => {
    const client = await signedIn();
    const first = await listInstanceWorkspaces(client);
    if (!first.next_page_results) {
      // Single-page instance: the second page is empty but still parses.
      const second = await listInstanceWorkspaces(client, first.next_cursor);
      expect(second.prev_page_results).toBe(true);
      return;
    }
    const second = await listInstanceWorkspaces(client, first.next_cursor);
    expect(second.results.length).toBeGreaterThan(0);
    expect(second.prev_page_results).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("instance workspace slug contract", () => {
  it("reports free, taken and restricted slugs", async () => {
    const client = await signedIn();
    await expect(checkInstanceWorkspaceSlug(client, `ct-never-created-${Date.now().toString(36)}`)).resolves.toEqual({
      status: true,
    });
    await expect(checkInstanceWorkspaceSlug(client, CONTRACT_WORKSPACE)).resolves.toEqual({ status: false });
    await expect(checkInstanceWorkspaceSlug(client, "api")).resolves.toEqual({ status: false });
  });

  it("rejects a missing slug with 400", async () => {
    const client = await signedIn();
    const error = await checkInstanceWorkspaceSlug(client, "").then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
  });
});

describe.skipIf(!contractEnabled)("instance workspace creation contract", () => {
  it("creates a workspace with the owner assigned server-side", async () => {
    const client = await signedIn();
    const slug = lifecycleSlug("ct-adm-ws");
    const created = await createInstanceWorkspace(client, {
      name: `Contract ${slug}`,
      slug,
      organization_size: "2-10",
    });
    try {
      expect(created.slug).toBe(slug);
      expect(created.organization_size).toBe("2-10");
      expect(created.owner?.email).toBe(CONTRACT_EMAIL);
      // The list read shows the new row with its counts.
      const page = await listInstanceWorkspaces(client);
      expect(page.results.some((row) => row.slug === slug)).toBe(true);
      await expect(checkInstanceWorkspaceSlug(client, slug)).resolves.toEqual({ status: false });
    } finally {
      // Cleanup through the member surface (the console offers no delete).
      const member = await sharedSignedIn();
      await member.delete(`/api/workspaces/${slug}/`).catch(() => null);
    }
  });

  it("refuses a duplicate slug", async () => {
    const client = await signedIn();
    const error = await createInstanceWorkspace(client, {
      name: "Dupe",
      slug: CONTRACT_WORKSPACE,
      organization_size: "2-10",
    }).then(
      () => null,
      (caught: unknown) => caught
    );
    expect(error).toBeInstanceOf(ApiError);
    expect([400, 409]).toContain((error as ApiError).status);
  });
});
