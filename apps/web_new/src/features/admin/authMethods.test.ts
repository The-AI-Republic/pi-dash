// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import {
  AUTH_METHOD_ENABLE_KEYS,
  AUTH_METHODS,
  canDisableAuthMethod,
  ENABLE_SIGNUP_KEY,
  isConfigFlagOn,
  isMethodConfigured,
  toConfigMap,
  type ConfigMap,
} from "./authMethods.js";

describe("AUTH_METHODS", () => {
  it("lists the six methods in the pinned fixed order", () => {
    expect(AUTH_METHODS.map((method) => method.name)).toEqual([
      "Unique codes",
      "Passwords",
      "Google",
      "GitHub",
      "GitLab",
      "Gitea",
    ]);
  });

  it("pins the per-method enable keys in registry order", () => {
    expect(AUTH_METHOD_ENABLE_KEYS).toEqual([
      "ENABLE_MAGIC_LINK_LOGIN",
      "ENABLE_EMAIL_PASSWORD",
      "IS_GOOGLE_ENABLED",
      "IS_GITHUB_ENABLED",
      "IS_GITLAB_ENABLED",
      "IS_GITEA_ENABLED",
    ]);
  });

  it("describes every card, noting the SMTP prerequisite on the magic-link card", () => {
    for (const method of AUTH_METHODS) {
      expect(method.description.length).toBeGreaterThan(0);
    }
    expect(AUTH_METHODS[0]?.description ?? "").toContain("SMTP");
  });

  it("keeps open-sign-up out of the guarded method keys", () => {
    expect(ENABLE_SIGNUP_KEY).toBe("ENABLE_SIGNUP");
    expect(AUTH_METHOD_ENABLE_KEYS).not.toContain(ENABLE_SIGNUP_KEY);
  });

  it("gives OAuth cards a provider slug and toggle cards none", () => {
    for (const method of AUTH_METHODS) {
      if (method.control === "oauth") {
        expect(method.providerSlug).toMatch(/^(google|github|gitlab|gitea)$/);
      } else {
        expect(method.providerSlug).toBeUndefined();
      }
    }
  });
});

describe("toConfigMap", () => {
  it("indexes rows by key, keeping null values", () => {
    const base = {
      id: "33333333-3333-4333-8333-333333333333",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      category: "AUTHENTICATION",
      is_encrypted: false,
      source: "db",
      is_managed: false,
    } as const;
    expect(
      toConfigMap([
        { ...base, key: "ENABLE_SIGNUP", value: "1" },
        { ...base, key: "GITHUB_CLIENT_ID", value: null },
      ])
    ).toEqual({ ENABLE_SIGNUP: "1", GITHUB_CLIENT_ID: null });
  });
});

describe("isConfigFlagOn", () => {
  it.each([
    ["1", true],
    ["0", false],
    ["", false],
    ["True", false],
    [null, false],
    [undefined, false],
  ])("reads %j as %s", (value, expected) => {
    expect(isConfigFlagOn(value)).toBe(expected);
  });
});

describe("isMethodConfigured", () => {
  const github = AUTH_METHODS.find((method) => method.key === "github");
  const gitea = AUTH_METHODS.find((method) => method.key === "gitea");
  const magicLink = AUTH_METHODS.find((method) => method.key === "unique-codes");
  if (!github || !gitea || !magicLink) throw new Error("registry lookup failed");

  it("requires the client id and secret pair", () => {
    expect(isMethodConfigured(github, { GITHUB_CLIENT_ID: "id", GITHUB_CLIENT_SECRET: "secret" })).toBe(true);
    expect(isMethodConfigured(github, { GITHUB_CLIENT_ID: "id", GITHUB_CLIENT_SECRET: "" })).toBe(false);
    expect(isMethodConfigured(github, { GITHUB_CLIENT_SECRET: "secret" })).toBe(false);
    expect(isMethodConfigured(github, {})).toBe(false);
  });

  it("treats null credential values as missing", () => {
    expect(isMethodConfigured(github, { GITHUB_CLIENT_ID: "id", GITHUB_CLIENT_SECRET: null })).toBe(false);
  });

  it("additionally requires the Gitea host", () => {
    expect(isMethodConfigured(gitea, { GITEA_CLIENT_ID: "id", GITEA_CLIENT_SECRET: "secret", GITEA_HOST: "" })).toBe(
      false
    );
    expect(
      isMethodConfigured(gitea, {
        GITEA_CLIENT_ID: "id",
        GITEA_CLIENT_SECRET: "secret",
        GITEA_HOST: "https://gitea.example.test",
      })
    ).toBe(true);
  });

  it("reads toggle-only cards as configured", () => {
    expect(isMethodConfigured(magicLink, {})).toBe(true);
  });
});

describe("canDisableAuthMethod", () => {
  const loneMagicLink: ConfigMap = {
    ENABLE_MAGIC_LINK_LOGIN: "1",
    ENABLE_EMAIL_PASSWORD: "0",
    IS_GOOGLE_ENABLED: "0",
    IS_GITHUB_ENABLED: "0",
    IS_GITLAB_ENABLED: "0",
    IS_GITEA_ENABLED: "0",
  };

  it("refuses to switch off the lone enabled method", () => {
    expect(canDisableAuthMethod("ENABLE_MAGIC_LINK_LOGIN", loneMagicLink)).toBe(false);
  });

  it("allows switching off one of several enabled methods", () => {
    expect(canDisableAuthMethod("ENABLE_MAGIC_LINK_LOGIN", { ...loneMagicLink, ENABLE_EMAIL_PASSWORD: "1" })).toBe(
      true
    );
  });

  it("exempts open-sign-up from the guard", () => {
    expect(canDisableAuthMethod(ENABLE_SIGNUP_KEY, { ...loneMagicLink, [ENABLE_SIGNUP_KEY]: "1" })).toBe(true);
  });

  it("exempts unknown keys", () => {
    expect(canDisableAuthMethod("ENABLE_SMTP", loneMagicLink)).toBe(true);
  });

  it("allows an already-off method flag", () => {
    expect(canDisableAuthMethod("ENABLE_EMAIL_PASSWORD", loneMagicLink)).toBe(true);
  });
});
