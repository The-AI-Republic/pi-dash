// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  describeEmailSendFailure,
  EMAIL_MESSAGES,
  emailCodeError,
  emailShapeError,
  emailStepOutcome,
  emailVerifyMessage,
} from "./email.js";

describe("email change rules", () => {
  it("checks the address shape and refuses the current address", () => {
    expect(emailShapeError("", "ada@example.com")).toBe(EMAIL_MESSAGES.required);
    expect(emailShapeError("not-an-email", "ada@example.com")).toBe(EMAIL_MESSAGES.invalid);
    expect(emailShapeError("Ada@Example.com", "ada@example.com")).toBe(EMAIL_MESSAGES.unchanged);
    expect(emailShapeError("new@example.com", "ada@example.com")).toBeNull();
  });

  it("requires a code on the second step", () => {
    expect(emailCodeError(" ")).toBe(EMAIL_MESSAGES.codeRequired);
    expect(emailCodeError("123456")).toBeNull();
  });

  it("maps first-step refusals to inline messages", () => {
    expect(describeEmailSendFailure({ body: { error: "An account with this email already exists" } })).toBe(
      EMAIL_MESSAGES.taken
    );
    expect(describeEmailSendFailure({ body: { error: "Invalid email format" } })).toBe(EMAIL_MESSAGES.invalid);
    expect(describeEmailSendFailure({ body: { error: "New email must be different from current email" } })).toBe(
      EMAIL_MESSAGES.unchanged
    );
    expect(describeEmailSendFailure({ message: "Rate limit" })).toBe("Rate limit");
    expect(describeEmailSendFailure({})).toBe(EMAIL_MESSAGES.failed);
    expect(describeEmailSendFailure(null)).toBe(EMAIL_MESSAGES.failed);
  });

  it("reports a taken address without mailing a code", () => {
    expect(emailStepOutcome({ existing: true })).toEqual({ kind: "taken", message: EMAIL_MESSAGES.taken });
    expect(emailStepOutcome({ existing: false, sendFailure: { body: { error: "Nope" } } })).toEqual({
      kind: "failed",
      message: "Nope",
    });
    expect(emailStepOutcome({ existing: false })).toEqual({ kind: "sent" });
  });

  it("maps verification refusals to the code field", () => {
    expect(emailVerifyMessage({ body: { error: "Invalid verification code" } })).toBe("Invalid verification code");
    expect(emailVerifyMessage({ message: "Verification code has expired or is invalid" })).toBe(
      "Verification code has expired or is invalid"
    );
    expect(emailVerifyMessage({})).toBe(EMAIL_MESSAGES.codeInvalid);
  });
});
