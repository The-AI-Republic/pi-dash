// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CONNECT_TAB_TITLE,
  CREDENTIAL_FORMAT_ERROR,
  CREDENTIAL_REQUIRED_ERROR,
  FALLBACK_CREDENTIAL_FIELD,
  appMechanism,
  cleanInstructions,
  cleanedBounceHref,
  connectReturnPath,
  dialogFields,
  hasBounceError,
  hasOutcomeParams,
  hubAppUrl,
  isSafeSetupUrl,
  isSecretField,
  reserveConnectTab,
  stripOutcomeParams,
  trimCredentialValues,
  validateCredentialValues,
} from "./lib/connect.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("appMechanism", () => {
  it("reads the key and none spellings exactly, everything else as provider (CLOUD-025)", () => {
    expect(appMechanism("api_key")).toBe("key");
    expect(appMechanism("none")).toBe("none");
    expect(appMechanism("oauth2")).toBe("provider");
    expect(appMechanism(undefined)).toBe("provider");
    expect(appMechanism("Api_Key")).toBe("provider");
  });
});

describe("reserveConnectTab", () => {
  function fakeTab(overrides: Record<string, unknown> = {}): Window {
    return {
      closed: false,
      document: { title: "" },
      close: vi.fn(),
      location: { href: "about:blank" },
      ...overrides,
    } as unknown as Window;
  }

  it("opens an attached placeholder tab synchronously (CLOUD-025)", () => {
    const tab = fakeTab({ opener: {} });
    const open = vi.fn(() => tab);
    const reserved = reserveConnectTab({ open } as unknown as Window);
    // No noopener: the flow steers and closes the tab it opened, which
    // needs the handle (NEWFRONT-662).
    expect(open).toHaveBeenCalledWith("about:blank", "_blank");
    expect(tab.opener).toBeNull();
    expect(reserved).toBe(tab);
    expect(tab.document.title).toBe(CONNECT_TAB_TITLE);
    expect(CONNECT_TAB_TITLE).toContain("Connecting");
  });

  it("reports a blocked popup as null instead of failing silently (CLOUD-025)", () => {
    const open = vi.fn(() => null);
    expect(reserveConnectTab({ open } as unknown as Window)).toBeNull();
  });

  it("treats an already-closed window as blocked", () => {
    const open = vi.fn(() => fakeTab({ closed: true }));
    expect(reserveConnectTab({ open } as unknown as Window)).toBeNull();
  });

  it("still reserves a tab that will not take a title", () => {
    const tab = fakeTab();
    Object.defineProperty(tab, "document", {
      get() {
        throw new Error("no document");
      },
    });
    const open = vi.fn(() => tab);
    expect(reserveConnectTab({ open } as unknown as Window)).toBe(tab);
  });
});

describe("stripOutcomeParams", () => {
  it("drops the error/connected pair and keeps the rest (CLOUD-026)", () => {
    expect(stripOutcomeParams("?error=openhub_unavailable&connected=1&kept=yes")).toBe("?kept=yes");
    expect(stripOutcomeParams("?error=openhub_unavailable")).toBe("");
    expect(stripOutcomeParams("?connected=1")).toBe("");
    expect(stripOutcomeParams("")).toBe("");
    expect(stripOutcomeParams("?q=notes")).toBe("?q=notes");
  });
});

describe("connectReturnPath", () => {
  it("carries the page minus any previous outcome, never the fragment (CLOUD-025)", () => {
    expect(connectReturnPath({ pathname: "/acme/apps/", search: "" })).toBe("/acme/apps/");
    expect(connectReturnPath({ pathname: "/acme/apps/", search: "?error=x&connected=1&kept=yes" })).toBe(
      "/acme/apps/?kept=yes"
    );
  });
});

describe("hasBounceError / hasOutcomeParams", () => {
  it("reports only on the error param but strips either outcome param (CLOUD-026)", () => {
    expect(hasBounceError("?error=openhub_unavailable")).toBe(true);
    expect(hasBounceError("?connected=1")).toBe(false);
    expect(hasBounceError("")).toBe(false);
    expect(hasOutcomeParams("?error=openhub_unavailable")).toBe(true);
    expect(hasOutcomeParams("?connected=1")).toBe(true);
    expect(hasOutcomeParams("?kept=yes")).toBe(false);
    expect(hasOutcomeParams("")).toBe(false);
  });
});

describe("cleanedBounceHref", () => {
  it("keeps the page and fragment, drops the outcome (CLOUD-026)", () => {
    expect(cleanedBounceHref({ pathname: "/acme/apps/", search: "?error=x&connected=1&kept=yes", hash: "" })).toBe(
      "/acme/apps/?kept=yes"
    );
    expect(cleanedBounceHref({ pathname: "/acme/apps/", search: "?error=x", hash: "#card" })).toBe("/acme/apps/#card");
  });
});

describe("hubAppUrl", () => {
  it("builds the detail page from the base plus the preferred slug (CLOUD-031)", () => {
    expect(hubAppUrl("https://hub.airepublic.com", "linear", "com.linear.linear")).toBe(
      "https://hub.airepublic.com/apps/linear"
    );
  });

  it("falls back to the platform id without a slug (CLOUD-031)", () => {
    expect(hubAppUrl("https://hub.airepublic.com/", "", "com.linear.linear")).toBe(
      "https://hub.airepublic.com/apps/com.linear.linear"
    );
  });

  it("keeps loopback navigable over plain HTTP (CLOUD-031)", () => {
    expect(hubAppUrl("http://127.0.0.1:4433/apps", "loop", "app-loop")).toBe("http://127.0.0.1:4433/apps/loop");
    expect(hubAppUrl("http://localhost:8080", "loop", "app-loop")).toBe("http://localhost:8080/apps/loop");
    expect(hubAppUrl("http://[::1]:8080", "loop", "app-loop")).toBe("http://[::1]:8080/apps/loop");
  });

  it("refuses missing, unparseable, credentialed, and unsafe bases (CLOUD-031)", () => {
    expect(hubAppUrl(undefined, "linear", "com.linear.linear")).toBeNull();
    expect(hubAppUrl("", "linear", "com.linear.linear")).toBeNull();
    expect(hubAppUrl("not-a-url", "linear", "com.linear.linear")).toBeNull();
    expect(hubAppUrl("https://user:pass@hub.example.com", "linear", "com.linear.linear")).toBeNull();
    expect(hubAppUrl("https://:pass@hub.example.com", "linear", "com.linear.linear")).toBeNull();
    expect(hubAppUrl("http://hub.example.com", "plain-http", "app-plain-http")).toBeNull();
    expect(hubAppUrl("ftp://hub.example.com", "linear", "com.linear.linear")).toBeNull();
  });
});

describe("isSafeSetupUrl", () => {
  it("links only absolute credentialless HTTPS addresses (CLOUD-027)", () => {
    expect(isSafeSetupUrl("https://crm.example.com/keys")).toBe(true);
    expect(isSafeSetupUrl("http://crm.example.com/keys")).toBe(false);
    expect(isSafeSetupUrl("https://user:pass@crm.example.com/keys")).toBe(false);
    expect(isSafeSetupUrl("https://:pass@crm.example.com/keys")).toBe(false);
    expect(isSafeSetupUrl("/relative/keys")).toBe(false);
    expect(isSafeSetupUrl(undefined)).toBe(false);
    expect(isSafeSetupUrl("")).toBe(false);
  });
});

describe("cleanInstructions", () => {
  it("drops emphasis markers and leaves markup for React to escape (CLOUD-027)", () => {
    expect(cleanInstructions("Paste your **API key** below.\n<script>alert(1)</script>")).toBe(
      "Paste your API key below.\n<script>alert(1)</script>"
    );
    expect(cleanInstructions(undefined)).toBe("");
    expect(cleanInstructions("Your api_key stays intact.")).toBe("Your api_key stays intact.");
  });
});

describe("dialogFields / isSecretField", () => {
  const manifest = [
    { key: "api_key", label: "API key", type: "secret", placeholder: "sk-...", validation: "^sk-" },
    { key: "region", label: "Region", type: "text", placeholder: "us" },
  ];

  it("builds fields from the manifest, or the single secret fallback (CLOUD-027)", () => {
    expect(dialogFields({ type: "api_key", status: "needs_auth", manualSetup: { fields: manifest } })).toBe(manifest);
    expect(dialogFields({ type: "api_key", status: "needs_auth" })).toEqual([FALLBACK_CREDENTIAL_FIELD]);
    expect(dialogFields({ type: "api_key", status: "needs_auth", manualSetup: {} })).toEqual([
      FALLBACK_CREDENTIAL_FIELD,
    ]);
    expect(dialogFields({ type: "api_key", status: "needs_auth", manualSetup: { fields: [] } })).toEqual([
      FALLBACK_CREDENTIAL_FIELD,
    ]);
    expect(FALLBACK_CREDENTIAL_FIELD).toEqual({ key: "api_key", label: "API key", type: "secret" });
  });

  it("masks secret fields only (CLOUD-027)", () => {
    expect(isSecretField(manifest[0]!)).toBe(true);
    expect(isSecretField(manifest[1]!)).toBe(false);
  });
});

describe("validateCredentialValues / trimCredentialValues", () => {
  const fields = [
    { key: "api_key", label: "API key", type: "secret", validation: "^sk-[a-z0-9]{4,}$" },
    { key: "region", label: "Region", type: "text" },
  ];

  it("surfaces every problem at once and blocks submit (CLOUD-027)", () => {
    expect(validateCredentialValues(fields, {})).toEqual({
      api_key: CREDENTIAL_REQUIRED_ERROR,
      region: CREDENTIAL_REQUIRED_ERROR,
    });
    expect(validateCredentialValues(fields, { api_key: "nope", region: "us" })).toEqual({
      api_key: CREDENTIAL_FORMAT_ERROR,
    });
    expect(validateCredentialValues(fields, { api_key: "sk-abcd99", region: "  " })).toEqual({
      region: CREDENTIAL_REQUIRED_ERROR,
    });
    expect(validateCredentialValues(fields, { api_key: "sk-abcd99", region: "us" })).toEqual({});
  });

  it("never lets an invalid hint pattern block the form (CLOUD-027)", () => {
    const broken = [{ key: "odd", label: "Odd", type: "text", validation: "([" }];
    expect(validateCredentialValues(broken, { odd: "anything" })).toEqual({});
    const emptyHint = [{ key: "plain", label: "Plain", type: "text", validation: "" }];
    expect(validateCredentialValues(emptyHint, { plain: "anything" })).toEqual({});
  });

  it("trims every value for submit (CLOUD-027)", () => {
    expect(trimCredentialValues(fields, { api_key: "  sk-abcd99  ", region: "us" })).toEqual({
      api_key: "sk-abcd99",
      region: "us",
    });
    expect(trimCredentialValues(fields, {})).toEqual({ api_key: "", region: "" });
  });
});
