// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  canSubmitNewPassword,
  describeForgotPasswordFailure,
  describeResetException,
  describeResetFailure,
  describeSetPasswordFailure,
  EXPIRED_TOKEN_BANNER,
  FORGOT_FALLBACK_MESSAGE,
  forgotSubmitLabel,
  forgotSuccessDescription,
  INVALID_TOKEN_BANNER,
  isUnknownAccountFailure,
  passwordMismatchError,
  resendCountdown,
  RESET_FALLBACK_MESSAGE,
  secretStrength,
  SET_FALLBACK_MESSAGE,
  weakSecretHint,
} from "./recovery.js";

function apiError(status: number, code: string, message: string): ApiError {
  return new ApiError({ status, code, message });
}

describe("forgotSuccessDescription", () => {
  it("names the address the link is heading to", () => {
    expect(forgotSuccessDescription("ada@example.com")).toBe("A password reset link is on its way to ada@example.com.");
  });
});

describe("resendCountdown", () => {
  it("is idle before the first send and after the cooldown elapses", () => {
    expect(resendCountdown(null, 100_000, 30)).toBe(0);
    expect(resendCountdown(100_000, 131_000, 30)).toBe(0);
  });

  it("counts whole seconds down from the cooldown", () => {
    expect(resendCountdown(100_000, 112_500, 30)).toBe(18);
  });
});

describe("forgotSubmitLabel", () => {
  it("counts down with singular and plural units", () => {
    expect(forgotSubmitLabel(12, true)).toBe("Resend in 12 seconds");
    expect(forgotSubmitLabel(1, true)).toBe("Resend in 1 second");
  });

  it("offers the first send, then resends", () => {
    expect(forgotSubmitLabel(0, false)).toBe("Send reset link");
    expect(forgotSubmitLabel(0, true)).toBe("Resend link");
  });
});

describe("isUnknownAccountFailure", () => {
  it("matches the unknown-account code and message only", () => {
    expect(isUnknownAccountFailure(apiError(400, "5060", "USER_DOES_NOT_EXIST"))).toBe(true);
    expect(isUnknownAccountFailure(apiError(400, "other", "USER_DOES_NOT_EXIST"))).toBe(true);
    expect(isUnknownAccountFailure(apiError(400, "5900", "RATE_LIMIT_EXCEEDED"))).toBe(false);
    expect(isUnknownAccountFailure(new Error("boom"))).toBe(false);
    expect(isUnknownAccountFailure(null)).toBe(false);
  });
});

describe("describeForgotPasswordFailure", () => {
  it("never leaks account existence for unknown addresses", () => {
    const message = describeForgotPasswordFailure(apiError(400, "5060", "USER_DOES_NOT_EXIST"));
    expect(message).toBe(FORGOT_FALLBACK_MESSAGE);
    expect(message).not.toMatch(/no account|does not exist|not found|unknown user/i);
  });

  it("explains the mail-less server in forgot-password terms", () => {
    expect(describeForgotPasswordFailure(apiError(400, "5025", "SMTP_NOT_CONFIGURED"))).toBe(
      "This server cannot send email. Contact your administrator."
    );
  });

  it("carries known server detail through the card matrix", () => {
    expect(describeForgotPasswordFailure(apiError(400, "5900", "RATE_LIMIT_EXCEEDED"))).toBe(
      "Rate limit exceeded. Wait a moment and try again."
    );
  });

  it("falls back to the raw detail, then to generic", () => {
    expect(describeForgotPasswordFailure(apiError(400, "9999", "Something bespoke"))).toBe("Something bespoke");
    expect(describeForgotPasswordFailure(apiError(400, "9999", ""))).toBe(FORGOT_FALLBACK_MESSAGE);
    expect(describeForgotPasswordFailure(apiError(0, "network", "fetch failed"))).toBe(FORGOT_FALLBACK_MESSAGE);
    expect(describeForgotPasswordFailure(new Error("boom"))).toBe(FORGOT_FALLBACK_MESSAGE);
  });
});

describe("describeSetPasswordFailure", () => {
  it("maps known codes and falls back to generic", () => {
    expect(describeSetPasswordFailure(apiError(400, "5145", "PASSWORD_ALREADY_SET"))).toBe(
      "A password is already set on this account."
    );
    expect(describeSetPasswordFailure(apiError(0, "network", "fetch failed"))).toBe(SET_FALLBACK_MESSAGE);
  });
});

describe("describeResetFailure", () => {
  it("marks dead links with the oracle-pinned banners", () => {
    expect(describeResetFailure({ code: "5125", message: "INVALID_PASSWORD_TOKEN" })).toEqual({
      message: INVALID_TOKEN_BANNER,
      tokenDead: true,
    });
    expect(describeResetFailure({ code: "5130", message: "EXPIRED_PASSWORD_TOKEN" })).toEqual({
      message: EXPIRED_TOKEN_BANNER,
      tokenDead: true,
    });
    expect(INVALID_TOKEN_BANNER).toMatch(/invalid password token/i);
    expect(EXPIRED_TOKEN_BANNER).toMatch(/expired password token/i);
  });

  it("keeps the form for retryable failures with the server message", () => {
    expect(describeResetFailure({ code: "5021", message: "PASSWORD_TOO_WEAK" })).toEqual({
      message: "That password is too weak. Choose a stronger one.",
      tokenDead: false,
    });
    expect(describeResetFailure({ code: "9999", message: "Something bespoke" })).toEqual({
      message: "Something bespoke",
      tokenDead: false,
    });
  });
});

describe("describeResetException", () => {
  it("treats a 5xx as the malformed-uid crash with the invalid-token banner", () => {
    expect(describeResetException(apiError(500, "http", "Internal Server Error"))).toEqual({
      message: INVALID_TOKEN_BANNER,
      tokenDead: true,
    });
  });

  it("keeps the form for anything else with detail or generic copy", () => {
    expect(describeResetException(apiError(400, "5021", "PASSWORD_TOO_WEAK"))).toEqual({
      message: "That password is too weak. Choose a stronger one.",
      tokenDead: false,
    });
    expect(describeResetException(apiError(0, "network", "fetch failed"))).toEqual({
      message: RESET_FALLBACK_MESSAGE,
      tokenDead: false,
    });
    expect(describeResetException(new Error("boom"))).toEqual({
      message: RESET_FALLBACK_MESSAGE,
      tokenDead: false,
    });
  });
});

describe("secretStrength", () => {
  it("reports empty and short secrets as not strong", () => {
    expect(secretStrength("")).toBe("empty");
    expect(secretStrength("Ab1!")).toBe("weak");
  });

  it("needs three of the four character classes", () => {
    expect(secretStrength("abcdefgh")).toBe("weak");
    expect(secretStrength("ABCDEFG1")).toBe("weak");
    expect(secretStrength("Abcdefgh")).toBe("weak");
    expect(secretStrength("Abcdefg1")).toBe("strong");
    expect(secretStrength("Abcdefg!")).toBe("strong");
    expect(secretStrength("Tq7!mZ2@kP9#xR")).toBe("strong");
  });
});

describe("weakSecretHint", () => {
  it("stays silent on empty and strong values", () => {
    expect(weakSecretHint("")).toBeNull();
    expect(weakSecretHint("Tq7!mZ2@kP9#xR")).toBeNull();
  });

  it("names the length rule first, then the mix rule", () => {
    expect(weakSecretHint("Ab1!")).toBe("Use at least 8 characters.");
    expect(weakSecretHint("abcdefgh")).toBe("Weak password — add mixed case, numbers, or symbols.");
  });
});

describe("passwordMismatchError", () => {
  it("stays silent while the confirmation is empty or matches", () => {
    expect(passwordMismatchError("secret-1", "", false)).toBeNull();
    expect(passwordMismatchError("secret-1", "secret-1", true)).toBeNull();
  });

  it("appears on blur or once the confirmation catches up in length", () => {
    expect(passwordMismatchError("secret-1", "sec", false)).toBeNull();
    expect(passwordMismatchError("secret-1", "sec", true)).toBe("The two passwords do not match.");
    expect(passwordMismatchError("secret-1", "secret-2", false)).toBe("The two passwords do not match.");
  });
});

describe("canSubmitNewPassword", () => {
  it("needs a strong secret with a matching confirmation", () => {
    expect(canSubmitNewPassword("Tq7!mZ2@kP9#xR", "Tq7!mZ2@kP9#xR")).toBe(true);
    expect(canSubmitNewPassword("abcdefgh", "abcdefgh")).toBe(false);
    expect(canSubmitNewPassword("Tq7!mZ2@kP9#xR", "Nb4@gH8$kL1!zQ")).toBe(false);
  });
});
