// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: current user and workspace membership against live Django.
import { describe, expect, it } from "vitest";
import { getMe, getMeSettings } from "../contracts/users.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { signIn } from "../contracts/auth.js";
import { CONTRACT_EMAIL, CONTRACT_PASSWORD, CONTRACT_WORKSPACE, contractClient, contractEnabled } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  const client = contractClient();
  const result = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
  expect(result.ok).toBe(true);
  return client;
}

describe.skipIf(!contractEnabled)("users contract", () => {
  it("reads me", async () => {
    const me = await getMe(await signedIn());
    expect(me.email).toBe(CONTRACT_EMAIL);
    expect(me.is_active).toBe(true);
  });

  it("reads me settings with a workspace pointer", async () => {
    const settings = await getMeSettings(await signedIn());
    const slugs = [settings.workspace.last_workspace_slug, settings.workspace.fallback_workspace_slug];
    expect(slugs).toContain(CONTRACT_WORKSPACE);
  });
});

describe.skipIf(!contractEnabled)("workspaces contract", () => {
  it("lists the workspaces the user belongs to", async () => {
    const workspaces = await listWorkspaces(await signedIn());
    const seeded = workspaces.find((workspace) => workspace.slug === CONTRACT_WORKSPACE);
    expect(seeded).toBeDefined();
    expect(seeded?.total_members).toBeGreaterThanOrEqual(1);
  });
});
