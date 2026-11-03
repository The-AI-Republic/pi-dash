// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: openhub catalog reads, install/auth/icon endpoints, and the
// profile namespaced app-settings flag against live cloud Django.
// Skipped without PIDASH_CONTRACT_CLOUD_BASE_URL: a scratch DEBUG cloud
// backend without a gateway (dev-login sessions, no Hub behind it).
// Hub-backed calls therefore surface the taxonomy as ApiErrors —
// asserted here — while status, icon validation, and the settings
// round-trip prove their success shapes live.
import { describe, expect, it } from "vitest";
import { ApiError } from "../errors.js";
import {
  getAppAuthStatus,
  getAppManifest,
  getOpenhubStatus,
  installApp,
  listInstalledApps,
  listMarketplaceApps,
  setAppActivation,
  startAppOAuth,
  submitAppApiKey,
  uninstallApp,
} from "../contracts/openhub.js";
import { agentToolsEnabled, getProfile, updateProfile } from "../contracts/profile.js";
import type { ApiClient } from "../client.js";
import { cloudContractEnabled, sharedCloudSignedIn } from "./setup.js";

async function signedIn(): Promise<ApiClient> {
  return sharedCloudSignedIn();
}

async function taxonomyOf(run: () => Promise<unknown>): Promise<ApiError> {
  const error = await run().catch((e: unknown) => e);
  expect(error).toBeInstanceOf(ApiError);
  return error as ApiError;
}

describe.skipIf(!cloudContractEnabled)("openhub contract", () => {
  it("reads the catalog gate shape", async () => {
    const status = await getOpenhubStatus(await signedIn());
    expect(typeof status.connected).toBe("boolean");
    expect(typeof status.has_gateway_scopes).toBe("boolean");
    expect(typeof status.hub_url).toBe("string");
  });

  it("rejects misshaped icon URLs before fetching", async () => {
    const client = await signedIn();
    for (const bad of ["http://cdn.example.com/icon.png", "not-a-url", ""]) {
      const error = await client.get("/api/openhub/apps/icon/", { query: { url: bad } }).then(
        () => null,
        (e: unknown) => e
      );
      expect(error).toBeInstanceOf(ApiError);
      expect((error as ApiError).status).toBe(400);
    }
  });

  it("round-trips the namespaced agent-tools flag", async () => {
    const client = await signedIn();
    const before = agentToolsEnabled((await getProfile(client)).settings);
    try {
      const flipped = await updateProfile(client, { settings: { openhub: { apps_enabled: !before } } });
      expect(agentToolsEnabled(flipped.settings)).toBe(!before);
      const reread = await getProfile(client);
      expect(agentToolsEnabled(reread.settings)).toBe(!before);
    } finally {
      const restored = await updateProfile(client, { settings: { openhub: { apps_enabled: before } } });
      expect(agentToolsEnabled(restored.settings)).toBe(before);
    }
  });

  it("refuses installs without a gateway through the taxonomy", async () => {
    const error = await taxonomyOf(async () => installApp(await signedIn(), "app-notes"));
    expect(error.status).toBe(401);
    expect(error.message).toBe("openhub_reauth_required");
  });

  it("rejects empty credential posts before any gateway call", async () => {
    const error = await taxonomyOf(async () => submitAppApiKey(await signedIn(), "app-manual", { fields: {} }));
    expect(error.status).toBe(400);
    expect(error.message).toBe("fields_required");
  });

  it("surfaces the taxonomy on the remaining Hub-backed calls", async () => {
    const client = await signedIn();
    const calls: Array<[string, () => Promise<unknown>]> = [
      ["marketplace", () => listMarketplaceApps(client, { limit: 24 })],
      ["installations", () => listInstalledApps(client)],
      ["manifest", () => getAppManifest(client, "app-notes")],
      ["uninstall", () => uninstallApp(client, "app-notes")],
      ["activation", () => setAppActivation(client, "app-notes", true)],
      ["auth-status", () => getAppAuthStatus(client, "app-notes")],
      ["oauth-start", () => startAppOAuth(client, "app-notes", "/acme/apps")],
      ["api-key", () => submitAppApiKey(client, "app-manual", { fields: { api_key: "sk-live" } })],
    ];
    for (const [name, run] of calls) {
      const error = await taxonomyOf(run);
      expect(error.status, name).toBeGreaterThanOrEqual(400);
      expect(error.message.length, name).toBeGreaterThan(0);
    }
  });
});
