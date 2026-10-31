// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: profile, user timezone and the timezone list against live Django.
import { describe, expect, it } from "vitest";
import { getProfile, updateProfile } from "../contracts/profile.js";
import { listTimezones, updateMyTimezone } from "../contracts/locale.js";
import { getMe } from "../contracts/users.js";
import { listWorkspaces } from "../contracts/workspaces.js";
import { CONTRACT_EMAIL, CONTRACT_WORKSPACE, contractEnabled, sharedSignedIn } from "./setup.js";
import type { ApiClient } from "../client.js";

async function signedIn(): Promise<ApiClient> {
  return sharedSignedIn();
}

describe.skipIf(!contractEnabled)("profile contract", () => {
  it("reads the profile with theme and language", async () => {
    const profile = await getProfile(await signedIn());
    expect(profile.language).toBe("en");
    expect(typeof profile.theme).toBe("object");
    expect(profile.is_app_rail_docked).toBe(true);
  });

  it("patches language and theme, then restores them", async () => {
    const client = await signedIn();
    const before = await getProfile(client);
    const patched = await updateProfile(client, {
      language: "fr",
      theme: { ...before.theme, contractProbe: "probe" },
    });
    expect(patched.language).toBe("fr");
    const restored = await updateProfile(client, { language: before.language, theme: before.theme });
    expect(restored.language).toBe(before.language);
  });

  it("patches the onboarding fields and the last workspace, then restores them", async () => {
    const client = await signedIn();
    const before = await getProfile(client);
    const workspaces = await listWorkspaces(client);
    const seeded = workspaces.find((workspace) => workspace.slug === CONTRACT_WORKSPACE);
    expect(seeded).toBeDefined();

    const patched = await updateProfile(client, {
      role: "designer",
      use_case: "planning",
      has_marketing_email_consent: !before.has_marketing_email_consent,
      onboarding_step: { ...before.onboarding_step, profile_complete: true },
      last_workspace_id: seeded?.id ?? null,
    });
    expect(patched.role).toBe("designer");
    expect(patched.use_case).toBe("planning");
    expect(patched.has_marketing_email_consent).toBe(!before.has_marketing_email_consent);
    expect(patched.last_workspace_id).toBe(seeded?.id);

    const restored = await updateProfile(client, {
      role: before.role,
      use_case: before.use_case,
      has_marketing_email_consent: before.has_marketing_email_consent,
      onboarding_step: before.onboarding_step,
      last_workspace_id: before.last_workspace_id,
    });
    expect(restored.role).toBe(before.role);
    expect(restored.use_case).toBe(before.use_case);
    expect(restored.last_workspace_id).toBe(before.last_workspace_id);
  });

  it("patches the user timezone and restores it", async () => {
    const client = await signedIn();
    const me = await getMe(client);
    const updated = await updateMyTimezone(client, "Europe/Paris");
    expect(updated.user_timezone).toBe("Europe/Paris");
    expect(updated.email).toBe(CONTRACT_EMAIL);
    const restored = await updateMyTimezone(client, me.user_timezone);
    expect(restored.user_timezone).toBe(me.user_timezone);
  });

  it("lists known timezones", async () => {
    const zones = await listTimezones(await signedIn());
    expect(zones.timezones.length).toBeGreaterThan(0);
    const paris = zones.timezones.find((zone) => zone.value === "Europe/Paris");
    expect(paris).toBeDefined();
    expect(paris?.utc_offset).toMatch(/^UTC/);
  });
});
