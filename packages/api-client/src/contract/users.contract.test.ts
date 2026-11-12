// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: current user and workspace membership against live Django.
import { describe, expect, it } from "vitest";
import { getMe, getMeSettings } from "../contracts/users.js";
import { markOnboarded, updateMe } from "../contracts/user-writes.js";
import { getProfile } from "../contracts/profile.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { CONTRACT_EMAIL, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
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

  it("patches the name and avatar, then restores them", async () => {
    const client = await signedIn();
    const before = await getMe(client);
    const updated = await updateMe(client, {
      first_name: "Contractual",
      last_name: "Test",
      avatar: "https://cdn.test/contract.png",
    });
    expect(updated.first_name).toBe("Contractual");
    expect(updated.avatar).toBe("https://cdn.test/contract.png");

    const readBack = await getMe(client);
    expect(readBack.first_name).toBe("Contractual");
    expect(readBack.avatar_url).toBe("https://cdn.test/contract.png");

    const restored = await updateMe(client, {
      first_name: before.first_name,
      last_name: before.last_name,
      avatar: before.avatar,
    });
    expect(restored.first_name).toBe(before.first_name);
    expect(restored.avatar).toBe(before.avatar);
  });

  it("stamps the onboard marker, then restores it", async () => {
    const client = await signedIn();
    const before = await getProfile(client);
    const marked = await markOnboarded(client, { is_onboarded: true });
    expect(marked).toEqual({ message: "Updated successfully" });
    const onboarded = await getProfile(client);
    expect(onboarded.is_onboarded).toBe(true);

    await markOnboarded(client, { is_onboarded: before.is_onboarded });
    const restored = await getProfile(client);
    expect(restored.is_onboarded).toBe(before.is_onboarded);
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
