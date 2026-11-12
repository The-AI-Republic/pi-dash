// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: spaces sign-in against live Django (NEWFRONT-752). Covers the
// email check, magic-code generation, the native form posts and their
// redirect error catalogue, and sign-out. Every account it creates uses a
// throwaway address so re-runs stay clean; the anonymous endpoints share
// the product's throttle, so the suite keeps its call count small.
import { describe, expect, it } from "vitest";
import { getMe } from "../contracts/users.js";
import {
  SPACE_AUTH_ERROR_CODES,
  checkSpaceEmail,
  generateSpaceMagicCode,
  parseSpaceAuthRedirect,
} from "../contracts/space-auth.js";
import { ApiError } from "../errors.js";
import { createClient } from "../client.js";
import { getCsrfToken } from "../contracts/auth-entry.js";
import {
  CONTRACT_BASE_URL,
  CONTRACT_EMAIL,
  CONTRACT_PASSWORD,
  contractClient,
  contractEnabled,
  contractTransport,
} from "./setup.js";

/**
 * The form views answer 302 to `/spaces/...`, a frontend path the
 * Django-only contract stack does not serve; following it would 404. Post
 * through a non-following transport and read the Location, which is the
 * exact value the browser's client ends up at.
 */
async function spaceFormLocation(
  path: string,
  form: Record<string, string>,
  jar = new Map<string, string>()
): Promise<string> {
  const transport = contractTransport(jar, { followRedirects: false });
  const client = createClient({ baseUrl: CONTRACT_BASE_URL, transport, validate: true });
  const token = (await getCsrfToken(client)).csrf_token;
  const body = new URLSearchParams(form).toString();
  const response = await transport(`${CONTRACT_BASE_URL}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded", "X-CSRFTOKEN": token },
    body,
    credentials: "include",
  });
  return response.headers.get("location") ?? "";
}

function throwaway(prefix: string): string {
  return `contract-space-${prefix}-${Date.now()}-${Math.floor(Math.random() * 1e6)}@example.com`;
}

describe.skipIf(!contractEnabled)("space auth contract", () => {
  it("routes a known address to its credential step", async () => {
    const parsed = await checkSpaceEmail(contractClient(), CONTRACT_EMAIL);
    expect(parsed.existing).toBe(true);
    expect(["MAGIC_CODE", "CREDENTIAL"]).toContain(parsed.status);
  });

  it("reports an unknown address without leaking existence", async () => {
    const parsed = await checkSpaceEmail(contractClient(), throwaway("unknown"));
    expect(parsed.existing).toBe(false);
  });

  it("refuses an empty address with the required-email code", async () => {
    const error = await checkSpaceEmail(contractClient(), "").catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(Error);
    // The JSON endpoint answers the {error_code, error_message} envelope.
    const record = error as { code?: unknown };
    expect(String(record.code)).toBe(String(SPACE_AUTH_ERROR_CODES.EMAIL_REQUIRED));
  });

  it("refuses a malformed address with the invalid-email code", async () => {
    const error = await checkSpaceEmail(contractClient(), "not-an-email").catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(Error);
    expect(String((error as { code?: unknown }).code)).toBe(String(SPACE_AUTH_ERROR_CODES.INVALID_EMAIL));
  });

  it("generates a magic code key for a known address", async () => {
    const parsed = await generateSpaceMagicCode(contractClient(), CONTRACT_EMAIL);
    expect(parsed.key).toContain(CONTRACT_EMAIL);
  });

  it("signs in with a password and reads the session back", async () => {
    const jar = new Map<string, string>();
    const location = await spaceFormLocation(
      "/auth/spaces/sign-in/",
      {
        email: CONTRACT_EMAIL,
        password: CONTRACT_PASSWORD,
        next_path: "/spaces/contract-board-anchor/",
      },
      jar
    );
    expect(location).not.toContain("error_code");
    expect(location).toContain("/spaces/contract-board-anchor/");
    const signedIn = createClient({
      baseUrl: CONTRACT_BASE_URL,
      transport: contractTransport(jar),
      validate: true,
    });
    const me = await getMe(signedIn);
    expect(me.email).toBe(CONTRACT_EMAIL);
  });

  it("refuses a missing password with the required-fields code", async () => {
    const location = await spaceFormLocation("/auth/spaces/sign-in/", { email: CONTRACT_EMAIL });
    const result = parseSpaceAuthRedirect(location);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe(String(SPACE_AUTH_ERROR_CODES.REQUIRED_EMAIL_PASSWORD_SIGN_IN));
      expect(result.location).toContain("/spaces/");
    }
  });

  it("refuses a wrong password with the authentication-failed code", async () => {
    const location = await spaceFormLocation("/auth/spaces/sign-in/", {
      email: CONTRACT_EMAIL,
      password: "definitely-not-the-password",
    });
    const result = parseSpaceAuthRedirect(location);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe(String(SPACE_AUTH_ERROR_CODES.AUTHENTICATION_FAILED_SIGN_IN));
    }
  });

  it("refuses an unknown address with the does-not-exist code", async () => {
    const location = await spaceFormLocation("/auth/spaces/sign-in/", {
      email: throwaway("missing"),
      password: "Anything123!xyz",
    });
    const result = parseSpaceAuthRedirect(location);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe(String(SPACE_AUTH_ERROR_CODES.USER_DOES_NOT_EXIST));
  });

  it("signs a new account up through the spaces form", async () => {
    const email = throwaway("signup");
    const jar = new Map<string, string>();
    const location = await spaceFormLocation(
      "/auth/spaces/sign-up/",
      {
        email,
        password: "SpacePass123!xyz",
        confirm_password: "SpacePass123!xyz",
        next_path: "/spaces/contract-board-anchor/",
      },
      jar
    );
    expect(location).not.toContain("error_code");
    const signedIn = createClient({
      baseUrl: CONTRACT_BASE_URL,
      transport: contractTransport(jar),
      validate: true,
    });
    const me = await getMe(signedIn);
    expect(me.email).toBe(email);
  });

  it("refuses a second sign-up with the already-registered code", async () => {
    const location = await spaceFormLocation("/auth/spaces/sign-up/", {
      email: CONTRACT_EMAIL,
      password: "Whatever123!xyz",
    });
    const result = parseSpaceAuthRedirect(location);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe(String(SPACE_AUTH_ERROR_CODES.USER_ALREADY_EXIST));
  });

  it("answers a missing magic code with the code-required error", async () => {
    const signInLocation = await spaceFormLocation("/auth/spaces/magic-sign-in/", {
      email: CONTRACT_EMAIL,
    });
    const signIn = parseSpaceAuthRedirect(signInLocation);
    expect(signIn.ok).toBe(false);
    if (!signIn.ok) {
      expect(signIn.code).toBe(String(SPACE_AUTH_ERROR_CODES.MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED));
    }
    const signUpLocation = await spaceFormLocation("/auth/spaces/magic-sign-up/", {
      email: throwaway("magic"),
    });
    const signUp = parseSpaceAuthRedirect(signUpLocation);
    expect(signUp.ok).toBe(false);
    if (!signUp.ok) {
      expect(signUp.code).toBe(String(SPACE_AUTH_ERROR_CODES.MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED));
    }
  });

  it("signs out and the session no longer reads back me", async () => {
    const jar = new Map<string, string>();
    await spaceFormLocation("/auth/spaces/sign-in/", { email: CONTRACT_EMAIL, password: CONTRACT_PASSWORD }, jar);
    const signedIn = createClient({
      baseUrl: CONTRACT_BASE_URL,
      transport: contractTransport(jar),
      validate: true,
    });
    expect((await getMe(signedIn)).email).toBe(CONTRACT_EMAIL);
    await spaceFormLocation("/auth/spaces/sign-out/", { next_path: "/spaces/" }, jar);
    const me = await getMe(
      createClient({ baseUrl: CONTRACT_BASE_URL, transport: contractTransport(jar), validate: true })
    ).catch((caught: unknown) => caught);
    expect(me).toBeInstanceOf(Error);
    expect(me).toBeInstanceOf(ApiError);
  });
});
