// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the spaces auth contracts: email check, magic generation,
// the form posts and the redirect error convention.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import { fakeResponse, recordingTransport } from "../client.test.js";
import {
  SPACE_AUTH_ERROR_CODES,
  SPACE_BASE_PATH,
  checkSpaceEmail,
  generateSpaceMagicCode,
  parseSpaceAuthRedirect,
  signInToSpace,
  signOutOfSpace,
  signUpToSpace,
  signUpToSpaceWithMagicCode,
  signInToSpaceWithMagicCode,
} from "./space-auth.js";

function clientFor(value: unknown, location?: string, status = 200) {
  const transport = recordingTransport((call) => {
    if (call.url.endsWith("/auth/get-csrf-token/")) {
      return fakeResponse({ body: JSON.stringify({ csrf_token: "csrf" }) });
    }
    // Redirect views answer 302; the transport's redirect walker hands
    // back the landing URL, and the test fake models that as a 200 with
    // the landing URL set (the same shape contracts.test.ts uses).
    return fakeResponse({
      status,
      body: value === undefined ? "<html>" : JSON.stringify(value),
      url: location ?? "https://api.test/",
    });
  });
  return { client: createClient({ baseUrl: "https://api.test", transport }), transport };
}

describe("space auth contracts", () => {
  it("checks an address and reports the next credential step", async () => {
    const { client, transport } = clientFor({ existing: true, status: "MAGIC_CODE" });
    const parsed = await checkSpaceEmail(client, "a@example.com");
    expect(parsed).toEqual({ existing: true, status: "MAGIC_CODE" });
    const call = transport.calls.at(-1);
    expect(call?.url).toBe("https://api.test/auth/spaces/email-check/");
    expect(call?.init.body).toBe(JSON.stringify({ email: "a@example.com" }));
  });

  it("rejects an unknown credential status", async () => {
    const { client } = clientFor({ existing: false, status: "SOMETHING" });
    await expect(checkSpaceEmail(client, "a@example.com")).rejects.toThrow();
  });

  it("generates a magic code and parses the key", async () => {
    const { client, transport } = clientFor({ key: "magic_a@example.com" });
    const parsed = await generateSpaceMagicCode(client, "a@example.com");
    expect(parsed.key).toBe("magic_a@example.com");
    expect(transport.calls.at(-1)?.url).toBe("https://api.test/auth/spaces/magic-generate/");
  });

  it("signs in through the native form post and follows a success redirect", async () => {
    const { client, transport } = clientFor(undefined, "https://api.test/spaces/acme/board");
    const result = await signInToSpace(client, {
      email: "a@example.com",
      password: "Secret123!",
      nextPath: "/spaces/acme/board",
    });
    expect(result).toEqual({ ok: true, location: "https://api.test/spaces/acme/board" });
    const call = transport.calls.at(-1);
    expect(call?.init.method).toBe("POST");
    expect(call?.init.body).toContain("email=a%40example.com");
    expect(call?.init.body).toContain("next_path=%2Fspaces%2Facme%2Fboard");
  });

  it("reads a refusal redirect as the error code and message", async () => {
    const { client } = clientFor(
      undefined,
      "https://api.test/spaces/?error_code=5065&error_message=AUTHENTICATION_FAILED_SIGN_IN"
    );
    const result = await signInToSpace(client, { email: "a@example.com", password: "bad" });
    expect(result).toEqual({
      ok: false,
      location: "https://api.test/spaces/?error_code=5065&error_message=AUTHENTICATION_FAILED_SIGN_IN",
      code: "5065",
      message: "AUTHENTICATION_FAILED_SIGN_IN",
    });
  });

  it("posts sign-up with its confirmation field", async () => {
    const { client, transport } = clientFor(undefined, "https://api.test/spaces/");
    await signUpToSpace(client, {
      email: "a@example.com",
      password: "Secret123!",
      confirmPassword: "Secret123!",
    });
    expect(transport.calls.at(-1)?.init.body).toContain("confirm_password=Secret123%21");
  });

  it("posts the magic code flows and sign-out", async () => {
    const signIn = clientFor(undefined, "https://api.test/spaces/");
    await signInToSpaceWithMagicCode(signIn.client, { email: "a@example.com", code: "123456" });
    expect(signIn.transport.calls.at(-1)?.url).toBe("https://api.test/auth/spaces/magic-sign-in/");
    expect(signIn.transport.calls.at(-1)?.init.body).toContain("code=123456");

    const signUp = clientFor(undefined, "https://api.test/spaces/");
    await signUpToSpaceWithMagicCode(signUp.client, { email: "a@example.com", code: "123456" });
    expect(signUp.transport.calls.at(-1)?.url).toBe("https://api.test/auth/spaces/magic-sign-up/");

    const out = clientFor(undefined, "https://api.test/spaces/");
    await signOutOfSpace(out.client, "/spaces/acme");
    expect(out.transport.calls.at(-1)?.url).toBe("https://api.test/auth/spaces/sign-out/");
  });

  it("parses a redirect with no error as success", () => {
    expect(parseSpaceAuthRedirect("/spaces/acme/board")).toEqual({
      ok: true,
      location: "/spaces/acme/board",
    });
  });

  it("treats an empty redirect target as a plain success", () => {
    expect(parseSpaceAuthRedirect("")).toEqual({ ok: true, location: "" });
  });

  it("falls back to the code when the redirect carries no error message", () => {
    expect(parseSpaceAuthRedirect("/spaces/?error_code=5065")).toEqual({
      ok: false,
      location: "/spaces/?error_code=5065",
      code: "5065",
      message: "5065",
    });
  });

  it("pins the spaces base path and the modelled error catalogue numbers", () => {
    expect(SPACE_BASE_PATH).toBe("/spaces/");
    expect(SPACE_AUTH_ERROR_CODES.REQUIRED_EMAIL_PASSWORD_SIGN_IN).toBe(5070);
    expect(SPACE_AUTH_ERROR_CODES.AUTHENTICATION_FAILED_SIGN_IN).toBe(5065);
    expect(SPACE_AUTH_ERROR_CODES.MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED).toBe(5085);
    expect(SPACE_AUTH_ERROR_CODES.PASSWORD_TOO_WEAK).toBe(5021);
    expect(SPACE_AUTH_ERROR_CODES.INVALID_EMAIL).toBe(5005);
    expect(SPACE_AUTH_ERROR_CODES.USER_ALREADY_EXIST).toBe(5030);
    expect(SPACE_AUTH_ERROR_CODES.OAUTH_NOT_CONFIGURED).toBe(5104);
    // Deliberate exclusions (parity/specs/space.md §5.4).
    expect(SPACE_AUTH_ERROR_CODES).not.toHaveProperty("RATE_LIMIT_EXCEEDED");
    expect(SPACE_AUTH_ERROR_CODES).not.toHaveProperty("GITEA_NOT_CONFIGURED");
  });
});
