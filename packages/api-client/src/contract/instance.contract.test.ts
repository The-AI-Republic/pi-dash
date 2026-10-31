// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: instance bootstrap + rename, admins session/list, admin
// auth posts, configurations, email disable/check, and the cloud probe
// against live Django. Skipped without PIDASH_CONTRACT_BASE_URL (see
// the runbook). The seed script provides the admin user/link, the
// configuration key rows, and repairs every fixture this suite moves.
// Mutating tests snapshot and restore in try/finally; the sign-up
// dance runs last because it unseats the seed admin link, which only
// re-seeding can restore (the API cannot reseat a soft-deleted link).
import { describe, expect, it } from "vitest";
import { getInstanceBootstrap } from "../contracts/auth-entry.js";
import {
  adminSignIn,
  adminSignOut,
  adminSignUp,
  checkEmailCredentials,
  disableEmailFeature,
  getAdminSession,
  listInstanceAdmins,
  listInstanceConfigurations,
  probeCloudAuthSession,
  updateInstance,
  updateInstanceConfigurations,
} from "../contracts/instance.js";
import { ApiError } from "../errors.js";
import {
  CONTRACT_ADMIN_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_SECONDARY_EMAIL,
  contractClient,
  contractEnabled,
  sharedAdminSignedIn,
  sharedSignedIn,
} from "./setup.js";

function throwawayAdminEmail(): string {
  const stamp = `${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  return `contract-admin-signup-${stamp}@example.com`;
}

function configMap(rows: Array<{ key: string; value: string | null }>): Map<string, string | null> {
  return new Map(rows.map((row) => [row.key, row.value]));
}

describe.skipIf(!contractEnabled)("instance bootstrap contract", () => {
  it("reads the config and instance halves anonymously", async () => {
    const bootstrap = await getInstanceBootstrap(contractClient());
    expect(Object.keys(bootstrap.config)).toHaveLength(10);
    for (const [key, value] of Object.entries(bootstrap.config)) {
      expect(typeof value, key).toBe("boolean");
    }
    expect(bootstrap.instance.is_setup_done).toBe(true);
    expect(bootstrap.instance.instance_name.length).toBeGreaterThan(0);
    expect(bootstrap.instance.instance_id.length).toBeGreaterThan(0);
    expect(bootstrap.instance.current_version.length).toBeGreaterThan(0);
    expect(bootstrap.instance.workspaces_exist).toBe(true);
  });

  it("renames the instance and flips telemetry, then restores both", async () => {
    const admin = await sharedAdminSignedIn();
    const before = await getInstanceBootstrap(admin);
    try {
      const updated = await updateInstance(admin, {
        instance_name: "contract-renamed",
        is_telemetry_enabled: !before.instance.is_telemetry_enabled,
      });
      expect(updated.instance_name).toBe("contract-renamed");
      expect(updated.is_telemetry_enabled).toBe(!before.instance.is_telemetry_enabled);
      const reread = await getInstanceBootstrap(admin);
      expect(reread.instance.instance_name).toBe("contract-renamed");
    } finally {
      await updateInstance(admin, {
        instance_name: before.instance.instance_name,
        is_telemetry_enabled: before.instance.is_telemetry_enabled,
      });
    }
    const restored = await getInstanceBootstrap(admin);
    expect(restored.instance.instance_name).toBe(before.instance.instance_name);
  });
});

describe.skipIf(!contractEnabled)("instance admin session contract", () => {
  it("reports anonymous without a session", async () => {
    await expect(getAdminSession(contractClient())).resolves.toEqual({ kind: "anonymous" });
  });

  it("reports anonymous for a member session", async () => {
    // Admin paths use a separate cookie: the member session the app
    // signs in with reads as anonymous to the admin check.
    await expect(getAdminSession(await sharedSignedIn())).resolves.toEqual({ kind: "anonymous" });
  });

  it("signs in as admin and the session reads back", async () => {
    const client = contractClient();
    const result = await adminSignIn(client, { email: CONTRACT_ADMIN_EMAIL, password: CONTRACT_PASSWORD });
    expect(result.ok).toBe(true);
    if (result.ok) expect(result.location).toContain("general");
    const session = await getAdminSession(client);
    expect(session.kind).toBe("admin");
    if (session.kind === "admin") expect(session.admin.email).toBe(CONTRACT_ADMIN_EMAIL);
  });

  it("rejects a wrong password and a non-admin alike with the failure code", async () => {
    // One client for both posts: neither establishes a session, and the
    // shared CSRF fetch keeps the anonymous throttle budget intact.
    const client = contractClient();
    const wrong = await adminSignIn(client, { email: CONTRACT_ADMIN_EMAIL, password: "wrong-password" });
    expect(wrong).toEqual({
      ok: false,
      location: expect.any(String),
      code: "5175",
      message: "ADMIN_AUTHENTICATION_FAILED",
    });
    // A valid member password for a non-admin fails exactly the same.
    // (The seed user holds an admin grant for the god-mode suites, so the
    // probe user — a plain member with the same seed password — stands in
    // as the non-admin here.)
    const nonAdmin = await adminSignIn(client, { email: CONTRACT_SECONDARY_EMAIL, password: CONTRACT_PASSWORD });
    expect(nonAdmin).toEqual({
      ok: false,
      location: expect.any(String),
      code: "5175",
      message: "ADMIN_AUTHENTICATION_FAILED",
    });
  });

  it("lists the admins with the seed admin among them", async () => {
    const admins = await listInstanceAdmins(await sharedAdminSignedIn());
    expect(admins.length).toBeGreaterThan(0);
    const emails = admins.map((entry) => entry.user_detail.email);
    expect(emails).toContain(CONTRACT_ADMIN_EMAIL);
    for (const entry of admins) {
      expect(entry.user_detail.email.length).toBeGreaterThan(0);
      expect(typeof entry.role).toBe("number");
    }
  });

  it("signs out and the session reads anonymous again", async () => {
    const client = contractClient();
    const signedIn = await adminSignIn(client, { email: CONTRACT_ADMIN_EMAIL, password: CONTRACT_PASSWORD });
    expect(signedIn.ok).toBe(true);
    const signedOut = await adminSignOut(client);
    expect(signedOut.location.length).toBeGreaterThan(0);
    await expect(getAdminSession(client)).resolves.toEqual({ kind: "anonymous" });
  });
});

describe.skipIf(!contractEnabled)("instance configuration contract", () => {
  it("lists the seeded key map", async () => {
    const rows = await listInstanceConfigurations(await sharedAdminSignedIn());
    const map = configMap(rows);
    for (const key of [
      "ENABLE_SMTP",
      "EMAIL_HOST",
      "EMAIL_PORT",
      "EMAIL_FROM",
      "ENABLE_SIGNUP",
      "ENABLE_MAGIC_LINK_LOGIN",
      "ENABLE_EMAIL_PASSWORD",
      "IS_GITHUB_ENABLED",
      "GITHUB_CLIENT_ID",
      "ENABLE_GITHUB_SYNC",
      "DISABLE_WORKSPACE_CREATION",
      "LLM_MODEL",
      "LLM_API_KEY",
    ]) {
      expect(map.has(key), key).toBe(true);
    }
    expect(map.get("EMAIL_HOST")).toBe("contract-smtp.invalid");
    expect(map.get("LLM_MODEL")).toBe("contract-model");
  });

  it("patches a partial map and restores it", async () => {
    const admin = await sharedAdminSignedIn();
    const before = configMap(await listInstanceConfigurations(admin));
    try {
      const echoed = await updateInstanceConfigurations(admin, {
        LLM_MODEL: "contract-model-probe",
        ENABLE_SIGNUP: "0",
      });
      expect(configMap(echoed).get("LLM_MODEL")).toBe("contract-model-probe");
      expect(configMap(echoed).get("ENABLE_SIGNUP")).toBe("0");
      const reread = configMap(await listInstanceConfigurations(admin));
      expect(reread.get("LLM_MODEL")).toBe("contract-model-probe");
      expect(reread.get("ENABLE_SIGNUP")).toBe("0");
    } finally {
      await updateInstanceConfigurations(admin, {
        LLM_MODEL: before.get("LLM_MODEL") ?? "",
        ENABLE_SIGNUP: before.get("ENABLE_SIGNUP") ?? "1",
      });
    }
    const restored = configMap(await listInstanceConfigurations(admin));
    expect(restored.get("LLM_MODEL")).toBe(before.get("LLM_MODEL"));
  });

  it("silently ignores unknown keys instead of creating them", async () => {
    const echoed = await updateInstanceConfigurations(await sharedAdminSignedIn(), {
      CONTRACT_NO_SUCH_KEY_XYZ: "1",
    });
    expect(echoed).toEqual([]);
  });

  it("disables the email feature and restores the stored values", async () => {
    const admin = await sharedAdminSignedIn();
    const before = configMap(await listInstanceConfigurations(admin));
    const blanked = ["EMAIL_HOST", "EMAIL_PORT", "EMAIL_FROM", "EMAIL_HOST_USER", "EMAIL_HOST_PASSWORD"];
    try {
      const status = await disableEmailFeature(admin);
      expect(status).toBe(200);
      const disabled = configMap(await listInstanceConfigurations(admin));
      expect(disabled.get("ENABLE_SMTP")).toBe("0");
      for (const key of blanked) {
        expect(disabled.get(key), key).toBe("");
      }
      // The transport-security pair is untouched by the disable.
      expect(disabled.get("EMAIL_USE_TLS")).toBe(before.get("EMAIL_USE_TLS"));
      expect(disabled.get("EMAIL_USE_SSL")).toBe(before.get("EMAIL_USE_SSL"));
    } finally {
      const restore: Record<string, string> = { ENABLE_SMTP: before.get("ENABLE_SMTP") ?? "1" };
      for (const key of blanked) {
        restore[key] = before.get(key) ?? "";
      }
      await updateInstanceConfigurations(admin, restore);
    }
    const restored = configMap(await listInstanceConfigurations(admin));
    expect(restored.get("ENABLE_SMTP")).toBe(before.get("ENABLE_SMTP"));
    expect(restored.get("EMAIL_HOST")).toBe(before.get("EMAIL_HOST"));
  });

  it("grades the stored mail config on a receiver-only check", async () => {
    // The stored host is unroutable, so the send fails deterministically
    // with the server's message. (A live positive needs real SMTP; the
    // success shape is proven by unit stubs.) The check grades the
    // stored configuration — the payload carries only the receiver.
    const error = await checkEmailCredentials(await sharedAdminSignedIn(), "probe@example.com").catch(
      (e: unknown) => e
    );
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
    expect((error as ApiError).message.length).toBeGreaterThan(0);
  });
});

describe.skipIf(!contractEnabled)("cloud auth probe contract", () => {
  it("resolves inconclusive on the OSS stack", async () => {
    // /api/auth/me/ is cloud-only: the OSS stack answers 404, which the
    // probe reads as inconclusive rather than anonymous.
    await expect(probeCloudAuthSession(contractClient())).resolves.toEqual({ kind: "inconclusive" });
    await expect(probeCloudAuthSession(await sharedAdminSignedIn())).resolves.toEqual({ kind: "inconclusive" });
  });
});

describe.skipIf(!contractEnabled)("instance admin sign-up contract", () => {
  it("refuses a second admin while one exists", async () => {
    const result = await adminSignUp(contractClient(), {
      firstName: "Second",
      email: throwawayAdminEmail(),
      password: "Contract-Admin-9f3k!Signup",
    });
    expect(result).toEqual({
      ok: false,
      location: expect.any(String),
      code: "5150",
      message: "ADMIN_ALREADY_EXIST",
    });
  });

  it("creates the first admin on an empty seat, then proves the revoked grant", async () => {
    // Runs last: it briefly soft-deletes every admin link (uncontracted
    // scaffolding — link management has no console caller) and proves
    // the setup post end to end. The seed link cannot be reseated
    // through the API (soft-deleted rows still hold the unique pair),
    // so re-seeding — not this test — restores the seed admin; the
    // runbook's seed step owns that. Dropping the throwaway link last
    // revokes the fresh grant mid-session, which the check must read
    // as forbidden: the live proof of the 403 mapping.
    const admin = await sharedAdminSignedIn();
    const before = await getInstanceBootstrap(admin);
    const links = await listInstanceAdmins(admin);
    const fresh = contractClient();
    const email = throwawayAdminEmail();
    let signedUp = false;
    try {
      for (const link of links) {
        const deleted = await admin.delete(`/api/instances/admins/${link.id}/`);
        expect(deleted.status).toBe(204);
      }
      const created = await adminSignUp(fresh, {
        firstName: "First",
        lastName: "Admin",
        email,
        companyName: "Dance Co",
        password: "Contract-Admin-9f3k!Signup",
        telemetryEnabled: false,
      });
      expect(created.ok).toBe(true);
      if (created.ok) expect(created.location).toContain("general");
      const session = await getAdminSession(fresh);
      expect(session.kind).toBe("admin");
      if (session.kind === "admin") expect(session.admin.email).toBe(email);
      const adopted = await getInstanceBootstrap(fresh);
      expect(adopted.instance.is_setup_done).toBe(true);
      expect(adopted.instance.instance_name).toBe("Dance Co");
      expect(adopted.instance.is_telemetry_enabled).toBe(false);
      signedUp = true;
    } finally {
      // Best-effort tidy only when the post landed: without a fresh
      // grant there is nothing to restore through the API, and an
      // early failure must surface its own error, not a cleanup one.
      // The soft-deleted seed link is always re-seed's to revive.
      if (signedUp) {
        await updateInstance(fresh, {
          instance_name: before.instance.instance_name,
          is_telemetry_enabled: before.instance.is_telemetry_enabled,
        }).catch(() => undefined);
        // Verify the restore while the grant is still admin: after the
        // drop below, only an anonymous re-read could do it, spending
        // throttle budget for no extra proof.
        const restored = await getInstanceBootstrap(fresh).catch(() => null);
        expect(restored?.instance.instance_name).toBe(before.instance.instance_name);
        expect(restored?.instance.is_telemetry_enabled).toBe(before.instance.is_telemetry_enabled);
        const current = await listInstanceAdmins(fresh).catch(() => []);
        for (const entry of current) {
          if (entry.user_detail.email === email) {
            await fresh.delete(`/api/instances/admins/${entry.id}/`).catch(() => undefined);
          }
        }
      }
    }
    await expect(getAdminSession(fresh)).resolves.toEqual({ kind: "forbidden" });
  });
});
