// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: profile, user timezone and the timezone list against live Django.
import { describe, expect, it } from "vitest";
import { getProfile, listTimezones, updateMyTimezone, updateProfile } from "../contracts/profile.js";
import { getMe } from "../contracts/users.js";
import { CONTRACT_EMAIL, contractEnabled, sharedSignedIn } from "./setup.js";
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
