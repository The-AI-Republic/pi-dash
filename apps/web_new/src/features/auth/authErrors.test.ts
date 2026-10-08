// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { describeSignInError } from "./authErrors.js";

describe("sign-in errors", () => {
  it("sends password failures back to the password step", () => {
    expect(describeSignInError("AUTHENTICATION_FAILED_SIGN_IN")).toEqual({
      message: expect.stringContaining("password"),
      step: "password",
    });
    expect(describeSignInError("USER_DOES_NOT_EXIST")?.step).toBe("password");
  });

  it("sends code failures back to the code step", () => {
    expect(describeSignInError("INVALID_MAGIC_CODE_SIGN_IN")?.step).toBe("code");
    expect(describeSignInError("EXPIRED_MAGIC_CODE_SIGN_IN")?.step).toBe("code");
    expect(describeSignInError("EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN")?.step).toBe("code");
  });

  it("falls back to a generic banner for unknown or missing codes", () => {
    expect(describeSignInError("SOMETHING_NEW")).toEqual({
      message: expect.any(String),
      step: "email",
    });
    expect(describeSignInError(null)).toBeNull();
    expect(describeSignInError(undefined)).toBeNull();
  });
});
