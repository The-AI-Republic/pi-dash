// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Contract: CSRF + sign-in against live Django. Skipped without
// PIDASH_CONTRACT_BASE_URL (see the runbook).
import { describe, expect, it } from "vitest";
import { checkEmail, generateMagicCode, getCsrfToken, signIn, signOut } from "../contracts/auth.js";
import { getMe } from "../contracts/users.js";
import { CONTRACT_EMAIL, CONTRACT_PASSWORD, contractClient, contractEnabled } from "./setup.js";

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
