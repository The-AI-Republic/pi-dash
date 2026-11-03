// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { describeAuthError, describeSubmitFailure } from "./authErrors.js";

describe("auth errors", () => {
  it("sends authentication failures to the password step in their mode", () => {
    expect(describeAuthError("5065")).toEqual({
      message: expect.stringMatching(/authentication failed/i),
      step: "password",
      mode: "sign-in",
    });
    expect(describeAuthError("5035")).toEqual({
      message: expect.stringMatching(/authentication failed/i),
      step: "password",
      mode: "sign-up",
    });
  });

  it("sends the magic-code family to the code step in its mode", () => {
    expect(describeAuthError("5090")).toMatchObject({ step: "code", mode: "sign-in" });
    expect(describeAuthError("5090")?.message).toMatch(/invalid magic code/i);
    expect(describeAuthError("5092")).toMatchObject({ step: "code", mode: "sign-up" });
    expect(describeAuthError("5092")?.message).toMatch(/invalid magic code/i);
    expect(describeAuthError("5080")).toMatchObject({ step: "code", mode: "sign-in" });
    expect(describeAuthError("5050")).toMatchObject({ step: "code", mode: "sign-up" });
    expect(describeAuthError("5095")?.message).toMatch(/expired magic code/i);
    expect(describeAuthError("5095")).toMatchObject({ step: "code", mode: "sign-in" });
    expect(describeAuthError("5097")).toMatchObject({ step: "code", mode: "sign-up" });
    expect(describeAuthError("5100")?.message).toMatch(/expired magic code/i);
    expect(describeAuthError("5100")).toMatchObject({ step: "code", mode: "sign-in" });
    expect(describeAuthError("5102")).toMatchObject({ step: "code", mode: "sign-up" });
  });

  it("pins the oracle email-step banners", () => {
    expect(describeAuthError("5060")).toMatchObject({ step: "email", mode: "sign-in" });
    expect(describeAuthError("5060")?.message).toMatch(/no account found/i);
    expect(describeAuthError("5900")).toMatchObject({ step: "email", mode: "sign-in" });
    expect(describeAuthError("5900")?.message).toMatch(/rate limit exceeded/i);
    expect(describeAuthError("5015")).toMatchObject({ step: "email", mode: "sign-in" });
    expect(describeAuthError("5015")?.message).toMatch(/sign up disabled/i);
  });

  it("carries address-preserving cross-links on the unknown/registered codes", () => {
    expect(describeAuthError("5060", "sign-in", "ada@example.com")).toMatchObject({
      action: { label: "Create an account", href: "/sign-up?email=ada%40example.com" },
    });
    expect(describeAuthError("5030", "sign-up", "ada@example.com")).toMatchObject({
      step: "email",
      mode: "sign-up",
      action: { label: "Sign in", href: "/?email=ada%40example.com" },
    });
  });

  it("lands every other recognized code on the email step in the home mode", () => {
    const cases: [string, RegExp][] = [
      ["5000", /not set up/i],
      ["5005", /not look valid/i],
      ["5010", /enter your email/i],
      ["5016", /code sign-in is disabled/i],
      ["5018", /password sign-in is disabled/i],
      ["5019", /deactivated/i],
      ["5020", /not valid/i],
      ["5021", /too weak/i],
      ["5025", /cannot send email/i],
      ["5040", /create an account/i],
      ["5045", /not look valid/i],
      ["5055", /emailed code/i],
      ["5070", /address and password/i],
      ["5075", /not look valid/i],
      ["5085", /emailed code/i],
      ["5104", /provider is not set up/i],
      ["5105", /google sign-in is not set up/i],
      ["5110", /github sign-in is not set up/i],
      ["5111", /gitlab sign-in is not set up/i],
      ["5112", /gitea sign-in is not set up/i],
      ["5115", /google sign-in failed/i],
      ["5120", /github sign-in failed/i],
      ["5121", /gitlab sign-in failed/i],
      ["5122", /not in the required organization/i],
      ["5123", /gitea sign-in failed/i],
      ["5125", /not valid/i],
      ["5130", /expired/i],
      ["5135", /did not match/i],
      ["5138", /enter your password/i],
      ["5140", /new password/i],
      ["5145", /already set/i],
    ];
    for (const [code, message] of cases) {
      expect(describeAuthError(code, "sign-up")).toMatchObject({ step: "email", mode: "sign-up" });
      expect(describeAuthError(code)?.message).toMatch(message);
    }
  });

  it("maps the admin range to the administrator banner", () => {
    for (const code of ["5150", "5155", "5175", "5190"]) {
      expect(describeAuthError(code)).toMatchObject({ step: "email", mode: "sign-in" });
      expect(describeAuthError(code)?.message).toMatch(/administrator/i);
    }
    expect(describeAuthError("5149")).toBeNull();
    expect(describeAuthError("5191")).toBeNull();
  });

  it("resolves submit failures through the matrix with the typed address", () => {
    expect(
      describeSubmitFailure({ code: "5065", message: "server words" }, "ada@example.com", {
        step: "password",
        mode: "sign-in",
      })
    ).toEqual({
      message: expect.stringMatching(/authentication failed/i),
      step: "password",
      mode: "sign-in",
      action: null,
    });
    expect(
      describeSubmitFailure({ code: "5030", message: "server words" }, "ada@example.com", {
        step: "password",
        mode: "sign-up",
      })
    ).toMatchObject({
      step: "email",
      mode: "sign-in",
      action: { label: "Sign in", href: "/?email=ada%40example.com" },
    });
  });

  it("falls back to the server message for unrecognized submit failures", () => {
    expect(
      describeSubmitFailure({ code: "9999", message: "Something odd happened." }, "ada@example.com", {
        step: "code",
        mode: "sign-up",
      })
    ).toEqual({ message: "Something odd happened.", step: "code", mode: "sign-up", action: null });
  });

  it("raises no banner for unknown or missing codes", () => {
    expect(describeAuthError("9999")).toBeNull();
    expect(describeAuthError("5056")).toBeNull();
    expect(describeAuthError("5999")).toBeNull();
    expect(describeAuthError("BOGUS")).toBeNull();
    expect(describeAuthError("")).toBeNull();
    expect(describeAuthError(null)).toBeNull();
    expect(describeAuthError(undefined)).toBeNull();
  });
});
