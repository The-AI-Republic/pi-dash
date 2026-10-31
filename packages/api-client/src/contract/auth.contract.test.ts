// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: CSRF + sign-in + sign-up + recovery + device + deactivation +
// instance config against live Django. Skipped without
// PIDASH_CONTRACT_BASE_URL (see the runbook). The seed script sets the
// EMAIL_HOST flag (forgot-password needs mail configured) and an autoset
// fixture user (set-password needs a passwordless-style account); tests
// that create accounts use unique throwaway addresses per run.
import { describe, expect, it } from "vitest";
import { deactivateAccount, getInstanceConfig, signOut } from "../contracts/auth.js";
import {
  approveDeviceCode,
  checkEmail,
  generateMagicCode,
  getCsrfToken,
  requestPasswordReset,
  resetPassword,
  setPassword,
  signIn,
  signUp,
  signUpWithMagicCode,
} from "../contracts/auth-entry.js";
import { getMe } from "../contracts/users.js";
import { ApiError } from "../errors.js";
import {
  CONTRACT_AUTOSET_EMAIL,
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  CONTRACT_WORKSPACE,
  contractClient,
  contractEnabled,
  sharedSignedIn,
} from "./setup.js";

function throwawayEmail(prefix: string): string {
  const stamp = `${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
  return `contract-${prefix}-${stamp}@example.com`;
}

describe.skipIf(!contractEnabled)("auth contract", () => {
  it("fetches a CSRF token", async () => {
    const parsed = await getCsrfToken(contractClient());
    expect(parsed.csrf_token.length).toBeGreaterThan(0);
  });

  it("rejects a wrong password with an error code", async () => {
    const result = await signIn(contractClient(), { email: CONTRACT_EMAIL, password: "wrong-password" });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code.length).toBeGreaterThan(0);
  });

  it("signs in and the session reads back me", async () => {
    const client = contractClient();
    const result = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
    expect(result).toEqual({ ok: true, location: expect.any(String) });
    const me = await getMe(client);
    expect(me.email).toBe(CONTRACT_EMAIL);
  });

  it("checks the seeded email and reports its login mode", async () => {
    const parsed = await checkEmail(contractClient(), CONTRACT_EMAIL);
    expect(parsed.existing).toBe(true);
    expect(["MAGIC_CODE", "CREDENTIAL"]).toContain(parsed.status);
  });

  it("checks an unknown email without leaking existence", async () => {
    const parsed = await checkEmail(contractClient(), "nobody-knows-this@example.com");
    expect(parsed.existing).toBe(false);
  });

  it("rejects a malformed email for code generation", async () => {
    const error = await generateMagicCode(contractClient(), "not-an-email").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(Error);
  });

  it("signs out and the session no longer reads back me", async () => {
    const client = contractClient();
    const signedIn = await signIn(client, { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD });
    expect(signedIn.ok).toBe(true);
    await signOut(client);
    const me = await getMe(client).catch((e: unknown) => e);
    expect(me).toBeInstanceOf(Error);
  });
});

describe.skipIf(!contractEnabled)("sign-up contract", () => {
  it("creates an account and signs it in", async () => {
    const client = contractClient();
    const email = throwawayEmail("signup");
    const result = await signUp(client, {
      email,
      password: "StrongPass123!xyz",
      confirmPassword: "StrongPass123!xyz",
    });
    expect(result.ok).toBe(true);
    const me = await getMe(client);
    expect(me.email).toBe(email);
  });

  it("refuses an existing address with the already-registered code", async () => {
    const result = await signUp(contractClient(), { email: CONTRACT_EMAIL, password: "whatever" });
    expect(result).toEqual({
      ok: false,
      location: expect.any(String),
      code: "5030",
      message: "USER_ALREADY_EXIST",
    });
  });

  it("answers an empty code with the required-fields code", async () => {
    // The view checks for a missing code before touching mail state, so
    // this pins the endpoint plus the redirect convention in any env.
    // (A live positive needs the mailed code; unit stubs prove the shape.)
    const result = await signUpWithMagicCode(contractClient(), {
      email: throwawayEmail("magic"),
      code: "",
    });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe("5055");
  });
});

describe.skipIf(!contractEnabled)("password recovery contract", () => {
  it("requests a reset link for a known address", async () => {
    const parsed = await requestPasswordReset(contractClient(), CONTRACT_EMAIL);
    expect(parsed.message.length).toBeGreaterThan(0);
  });

  it("answers an unknown reset token with the invalid-token code", async () => {
    // A decodable uid for a user that does not exist: deterministic in
    // any env (a live positive needs the mailed link).
    const uid = Buffer.from("123e4567-e89b-12d3-a456-426614174000").toString("base64url");
    const result = await resetPassword(contractClient(), {
      uid,
      token: "bogus-token",
      password: "NewStrong123!xyz",
    });
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe("5125");
  });

  it("refuses a second password with the already-set code", async () => {
    const error = await setPassword(await sharedSignedIn(), "AnotherPass123!xyz").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(400);
    expect((error as ApiError).code).toBe("5145");
  });

  it("sets the first password for a passwordless-style account", async () => {
    // Consumes the autoset seed fixture; re-running seed-contracts.py
    // --apply restores it for the next run.
    const client = contractClient();
    const signedIn = await signIn(client, { email: CONTRACT_AUTOSET_EMAIL, password: CONTRACT_PASSWORD });
    expect(signedIn.ok).toBe(true);
    const updated = await setPassword(client, "FirstSetPass123!xyz");
    expect(updated.email).toBe(CONTRACT_AUTOSET_EMAIL);
    expect(updated.is_password_autoset).toBe(false);
    const relogin = await signIn(contractClient(), {
      email: CONTRACT_AUTOSET_EMAIL,
      password: "FirstSetPass123!xyz",
    });
    expect(relogin.ok).toBe(true);
  });
});

describe.skipIf(!contractEnabled)("device approval contract", () => {
  it("approves a fresh device code, idempotently", async () => {
    const client = await sharedSignedIn();
    // device/start is the CLI's call, not the web UI's, so it stays
    // uncontracted scaffolding: mint a code, then prove the approval.
    const started = await client.sendJson<{ user_code: string }>("POST", "/api/v1/auth/device/start/", {
      json: {},
    });
    expect(typeof started.data.user_code).toBe("string");
    const grant = await approveDeviceCode(client, started.data.user_code);
    expect(grant.ok).toBe(true);
    expect(grant.user_email).toBe(CONTRACT_EMAIL);
    expect(grant.workspace_slug).toBe(CONTRACT_WORKSPACE);
    const again = await approveDeviceCode(client, started.data.user_code);
    expect(again.ok).toBe(true);
  });

  it("rejects an unknown code", async () => {
    const error = await approveDeviceCode(await sharedSignedIn(), "ZZZZ-9999").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expect((error as ApiError).message.length).toBeGreaterThan(0);
  });
});

describe.skipIf(!contractEnabled)("deactivation contract", () => {
  it("deactivates a throwaway account and ends its session", async () => {
    const client = contractClient();
    const created = await signUp(client, {
      email: throwawayEmail("deactivate"),
      password: "StrongPass123!xyz",
    });
    expect(created.ok).toBe(true);
    const status = await deactivateAccount(client);
    expect(status).toBe(204);
    const me = await getMe(client).catch((e: unknown) => e);
    expect(me).toBeInstanceOf(Error);
  });
});

describe.skipIf(!contractEnabled)("instance config contract", () => {
  it("reads the auth flags", async () => {
    const config = await getInstanceConfig(contractClient());
    expect(Object.keys(config)).toHaveLength(12);
    for (const [key, value] of Object.entries(config)) {
      const expected = key === "instance_changelog_url" ? "string" : key === "file_size_limit" ? "number" : "boolean";
      expect(typeof value, key).toBe(expected);
    }
    expect(config.is_smtp_configured).toBe(true);
    expect(config.is_email_password_enabled).toBe(true);
    expect(config.file_size_limit).toBeGreaterThan(0);
  });
});
