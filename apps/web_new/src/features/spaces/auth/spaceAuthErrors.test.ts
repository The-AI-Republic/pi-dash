// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { SPACE_AUTH_ERROR_CODES } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import type { SpaceAuthMode, SpaceAuthStep } from "./spaceAuthMachine.js";
import {
  SPACE_AUTH_REQUEST_FAILED_MESSAGE,
  bannerForSpaceAuthFailure,
  describeSpaceAuthCode,
} from "./spaceAuthErrors.js";

interface ExpectedMapping {
  code: string;
  message: string;
  step?: SpaceAuthStep | undefined;
  mode?: SpaceAuthMode | undefined;
}

const EXPECTED: ExpectedMapping[] = [
  { code: "5000", message: "This instance isn't set up yet. Contact your administrator." },
  { code: "5005", message: "That email address doesn't look right." },
  { code: "5010", message: "Enter your email address." },
  { code: "5015", message: "Creating new accounts is disabled on this instance. Contact your administrator." },
  { code: "5019", message: "This account has been deactivated. Contact your administrator." },
  { code: "5020", message: "That password isn't valid. Try again.", step: "password" },
  {
    code: "5025",
    message: "This instance can't send email, so codes are unavailable. Contact your administrator.",
  },
  { code: "5030", message: "This address is already registered. Sign in with it instead.", step: "email" },
  {
    code: "5035",
    message: "Sign-up failed. Check your details and try again.",
    step: "password",
    mode: "sign-up",
  },
  { code: "5040", message: "Enter your email address and password to create an account." },
  { code: "5045", message: "That email address doesn't look right." },
  { code: "5050", message: "That email address doesn't look right." },
  { code: "5055", message: "Enter the code from your email.", step: "code", mode: "sign-up" },
  { code: "5060", message: "No account uses this address. Create one to get started.", step: "email" },
  {
    code: "5065",
    message: "Sign-in failed. Check your password and try again.",
    step: "password",
    mode: "sign-in",
  },
  { code: "5070", message: "Enter your email address and password to sign in." },
  { code: "5075", message: "That email address doesn't look right." },
  { code: "5080", message: "That email address doesn't look right." },
  { code: "5085", message: "Enter the code from your email.", step: "code", mode: "sign-in" },
  {
    code: "5090",
    message: "That code isn't valid. Check your email and try again.",
    step: "code",
    mode: "sign-in",
  },
  {
    code: "5092",
    message: "That code isn't valid. Check your email and try again.",
    step: "code",
    mode: "sign-up",
  },
  {
    code: "5095",
    message: "That code has expired. Send another code and try again.",
    step: "code",
    mode: "sign-in",
  },
  {
    code: "5097",
    message: "That code has expired. Send another code and try again.",
    step: "code",
    mode: "sign-up",
  },
  {
    code: "5100",
    message: "That code has expired. Send another code and try again.",
    step: "code",
    mode: "sign-in",
  },
  {
    code: "5102",
    message: "That code has expired. Send another code and try again.",
    step: "code",
    mode: "sign-up",
  },
  { code: "5104", message: "Provider sign-in isn't configured. Contact your administrator." },
  { code: "5105", message: "Google sign-in isn't configured. Contact your administrator." },
  { code: "5110", message: "GitHub sign-in isn't configured. Contact your administrator." },
  { code: "5111", message: "GitLab sign-in isn't configured. Contact your administrator." },
  { code: "5115", message: "Google couldn't sign you in. Try again." },
  { code: "5120", message: "GitHub couldn't sign you in. Try again." },
  { code: "5121", message: "GitLab couldn't sign you in. Try again." },
  { code: "5122", message: "Your GitHub account isn't in the required organization." },
];

describe("describeSpaceAuthCode", () => {
  it.each(EXPECTED.map((entry) => [entry.code, entry] as const))(
    "maps %s to its banner and fixing step",
    (_code, expected) => {
      expect(describeSpaceAuthCode(expected.code)).toEqual({
        message: expected.message,
        ...(expected.step === undefined ? {} : { step: expected.step }),
        ...(expected.mode === undefined ? {} : { mode: expected.mode }),
      });
    }
  );

  it("accepts numeric codes and surrounding whitespace", () => {
    expect(describeSpaceAuthCode(5065)?.message).toBe("Sign-in failed. Check your password and try again.");
    expect(describeSpaceAuthCode("  5065  ")?.step).toBe("password");
  });

  it.each([["5900"], ["5112"], ["5021"], ["9999"], [""], ["banana"]])("leaves %s unmapped", (code) => {
    expect(describeSpaceAuthCode(code)).toBeNull();
  });

  it("models exactly the contract's codes except the client-gated weak password", () => {
    const contractNumbers = new Set(
      Object.entries(SPACE_AUTH_ERROR_CODES)
        .filter(([name]) => name !== "PASSWORD_TOO_WEAK")
        .map(([, number]) => String(number))
    );
    const catalogNumbers = new Set(EXPECTED.map((entry) => entry.code));
    expect(catalogNumbers).toEqual(contractNumbers);
  });
});

describe("bannerForSpaceAuthFailure", () => {
  it("reads numeric codes from the catalog", () => {
    expect(bannerForSpaceAuthFailure("5102")).toBe("That code has expired. Send another code and try again.");
    expect(bannerForSpaceAuthFailure("5900")).toBeNull();
  });

  it("explains transport failures instead of stranding the visitor", () => {
    for (const code of ["network", "timeout", "http", ""]) {
      expect(bannerForSpaceAuthFailure(code)).toBe(SPACE_AUTH_REQUEST_FAILED_MESSAGE);
    }
  });
});
