// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  BLANKED_MAIL_KEYS,
  buildSmtpPatch,
  emailFormValuesFromConfig,
  formatConfigFlag,
  isEmailFormDirty,
  isEmailFormValid,
  parseConfigFlag,
  securityChoiceFromFlags,
  securityFlagsFromChoice,
  SMTP_ENABLED_KEY,
  SMTP_FROM_KEY,
  SMTP_HOST_KEY,
  SMTP_PASSWORD_KEY,
  SMTP_PORT_KEY,
  SMTP_SSL_KEY,
  SMTP_TLS_KEY,
  SMTP_USER_KEY,
  type EmailFormValues,
} from "./emailConfig.js";

function validValues(overrides: Partial<EmailFormValues> = {}): EmailFormValues {
  return {
    host: "smtp.example.com",
    port: "587",
    from: "team@example.com",
    user: "",
    password: "",
    security: "tls",
    ...overrides,
  };
}

describe("parseConfigFlag", () => {
  it("reads 1 and other nonzero numbers as on", () => {
    expect(parseConfigFlag("1")).toBe(true);
    expect(parseConfigFlag("2")).toBe(true);
    expect(parseConfigFlag(" 1 ")).toBe(true);
  });

  it("reads 0, blank, missing, and non-numeric values as off", () => {
    expect(parseConfigFlag("0")).toBe(false);
    expect(parseConfigFlag("0.0")).toBe(false);
    expect(parseConfigFlag("")).toBe(false);
    expect(parseConfigFlag("   ")).toBe(false);
    expect(parseConfigFlag(null)).toBe(false);
    expect(parseConfigFlag(undefined)).toBe(false);
    expect(parseConfigFlag("yes")).toBe(false);
  });
});

describe("formatConfigFlag", () => {
  it("writes flags back as 1/0 strings", () => {
    expect(formatConfigFlag(true)).toBe("1");
    expect(formatConfigFlag(false)).toBe("0");
  });
});

describe("securityChoiceFromFlags", () => {
  it("derives TLS from the TLS flag", () => {
    expect(securityChoiceFromFlags("1", "0")).toBe("tls");
  });

  it("derives SSL from the SSL flag", () => {
    expect(securityChoiceFromFlags("0", "1")).toBe("ssl");
  });

  it("defaults to none when neither flag is on", () => {
    expect(securityChoiceFromFlags("0", "0")).toBe("none");
    expect(securityChoiceFromFlags(undefined, null)).toBe("none");
  });

  it("lets TLS win the both-on tie", () => {
    expect(securityChoiceFromFlags("1", "1")).toBe("tls");
  });
});

describe("securityFlagsFromChoice", () => {
  it("maps TLS, SSL, and none onto the flag pairs 10/01/00", () => {
    expect(securityFlagsFromChoice("tls")).toEqual({ [SMTP_TLS_KEY]: "1", [SMTP_SSL_KEY]: "0" });
    expect(securityFlagsFromChoice("ssl")).toEqual({ [SMTP_TLS_KEY]: "0", [SMTP_SSL_KEY]: "1" });
    expect(securityFlagsFromChoice("none")).toEqual({ [SMTP_TLS_KEY]: "0", [SMTP_SSL_KEY]: "0" });
  });
});

describe("emailFormValuesFromConfig", () => {
  it("reads the stored mail values verbatim", () => {
    expect(
      emailFormValuesFromConfig({
        [SMTP_HOST_KEY]: "smtp.example.com",
        [SMTP_PORT_KEY]: "587",
        [SMTP_FROM_KEY]: "team@example.com",
        [SMTP_USER_KEY]: "mailer",
        [SMTP_PASSWORD_KEY]: "secret",
        [SMTP_TLS_KEY]: "0",
        [SMTP_SSL_KEY]: "1",
      })
    ).toEqual({
      host: "smtp.example.com",
      port: "587",
      from: "team@example.com",
      user: "mailer",
      password: "secret",
      security: "ssl",
    });
  });

  it("reads null rows and missing keys as blank with no security", () => {
    expect(emailFormValuesFromConfig({ [SMTP_PORT_KEY]: null })).toEqual({
      host: "",
      port: "",
      from: "",
      user: "",
      password: "",
      security: "none",
    });
  });
});

describe("isEmailFormValid", () => {
  it("requires host, port, and sender address", () => {
    expect(isEmailFormValid(validValues())).toBe(true);
    expect(isEmailFormValid(validValues({ host: "" }))).toBe(false);
    expect(isEmailFormValid(validValues({ port: "" }))).toBe(false);
    expect(isEmailFormValid(validValues({ from: "" }))).toBe(false);
  });

  it("treats whitespace-only required fields as blank", () => {
    expect(isEmailFormValid(validValues({ host: "   " }))).toBe(false);
    expect(isEmailFormValid(validValues({ port: " " }))).toBe(false);
    expect(isEmailFormValid(validValues({ from: " " }))).toBe(false);
  });

  it("never blocks on the optional auth fields or the security choice", () => {
    expect(isEmailFormValid(validValues({ user: "", password: "" }))).toBe(true);
    expect(isEmailFormValid(validValues({ user: "mailer", password: "secret", security: "none" }))).toBe(true);
  });
});

describe("isEmailFormDirty", () => {
  it("reads identical values as clean", () => {
    expect(isEmailFormDirty(validValues(), validValues())).toBe(false);
  });

  it("dirties on any text field change", () => {
    const initial = validValues();
    expect(isEmailFormDirty({ ...initial, host: "other.example.com" }, initial)).toBe(true);
    expect(isEmailFormDirty({ ...initial, port: "465" }, initial)).toBe(true);
    expect(isEmailFormDirty({ ...initial, from: "other@example.com" }, initial)).toBe(true);
    expect(isEmailFormDirty({ ...initial, user: "mailer" }, initial)).toBe(true);
    expect(isEmailFormDirty({ ...initial, password: "secret" }, initial)).toBe(true);
  });

  it("dirties on a security-only flip (NEWFRONT-478 intent)", () => {
    const initial = validValues({ security: "tls" });
    expect(isEmailFormDirty({ ...initial, security: "ssl" }, initial)).toBe(true);
    expect(isEmailFormDirty({ ...initial, security: "none" }, initial)).toBe(true);
  });
});

describe("buildSmtpPatch", () => {
  it("carries the mail keys plus the security flags and forces SMTP on", () => {
    expect(buildSmtpPatch(validValues({ user: "mailer", password: "secret", security: "ssl" }))).toEqual({
      [SMTP_HOST_KEY]: "smtp.example.com",
      [SMTP_PORT_KEY]: "587",
      [SMTP_FROM_KEY]: "team@example.com",
      [SMTP_USER_KEY]: "mailer",
      [SMTP_PASSWORD_KEY]: "secret",
      [SMTP_TLS_KEY]: "0",
      [SMTP_SSL_KEY]: "1",
      [SMTP_ENABLED_KEY]: "1",
    });
  });
});

describe("BLANKED_MAIL_KEYS", () => {
  it("covers the mail values plus the master flag but keeps the security pair", () => {
    expect(BLANKED_MAIL_KEYS.has(SMTP_HOST_KEY)).toBe(true);
    expect(BLANKED_MAIL_KEYS.has(SMTP_PORT_KEY)).toBe(true);
    expect(BLANKED_MAIL_KEYS.has(SMTP_USER_KEY)).toBe(true);
    expect(BLANKED_MAIL_KEYS.has(SMTP_PASSWORD_KEY)).toBe(true);
    expect(BLANKED_MAIL_KEYS.has(SMTP_FROM_KEY)).toBe(true);
    expect(BLANKED_MAIL_KEYS.has(SMTP_ENABLED_KEY)).toBe(true);
    expect(BLANKED_MAIL_KEYS.has(SMTP_TLS_KEY)).toBe(false);
    expect(BLANKED_MAIL_KEYS.has(SMTP_SSL_KEY)).toBe(false);
  });
});
