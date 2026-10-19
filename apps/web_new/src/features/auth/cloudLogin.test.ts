// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  clearExplicitLogout,
  cloudHandoffTarget,
  cloudSignInUrl,
  CLOUD_SIGN_IN_HREF,
  EXPLICIT_LOGOUT_KEY,
  markExplicitLogout,
  parseCloudLoginSearch,
  readExplicitLogout,
  resolveCloudReturnTarget,
  shouldAttemptSilentSignIn,
} from "./cloudLogin.js";

const ORIGIN = "https://app.test";

function memoryStorage(initial: Record<string, string> = {}) {
  const entries = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => void entries.set(key, value),
    removeItem: (key: string) => void entries.delete(key),
    entries,
  };
}

describe("parseCloudLoginSearch", () => {
  it("reads the target and the error", () => {
    expect(parseCloudLoginSearch("?next_path=%2Fdocs&error=oidc_down")).toEqual({
      nextPath: "/docs",
      error: "oidc_down",
    });
  });

  it("treats blank values as absent", () => {
    expect(parseCloudLoginSearch("?next_path=&error=")).toEqual({ nextPath: null, error: null });
    expect(parseCloudLoginSearch("")).toEqual({ nextPath: null, error: null });
  });

  it("accepts the query without a leading question mark", () => {
    expect(parseCloudLoginSearch("next_path=%2Facme")).toEqual({ nextPath: "/acme", error: null });
  });
});

describe("resolveCloudReturnTarget", () => {
  it("keeps an app-local path and query", () => {
    expect(resolveCloudReturnTarget("/acme/projects/p/issues?tab=comments", ORIGIN)).toBe(
      "/acme/projects/p/issues?tab=comments"
    );
  });

  it("drops the fragment", () => {
    expect(resolveCloudReturnTarget("/docs#install", ORIGIN)).toBe("/docs");
  });

  it("rejects protocol-relative, absolute, backslashed and relative values", () => {
    for (const unsafe of [
      "//evil.example/steal",
      "https://evil.example/steal",
      "http://localhost:13038/downgrade",
      "/\\evil.example/steal",
      "workspace/issues/1",
    ]) {
      expect(resolveCloudReturnTarget(unsafe, ORIGIN)).toBeNull();
    }
  });

  it("rejects missing values", () => {
    expect(resolveCloudReturnTarget(null, ORIGIN)).toBeNull();
    expect(resolveCloudReturnTarget(undefined, ORIGIN)).toBeNull();
    expect(resolveCloudReturnTarget("", ORIGIN)).toBeNull();
  });

  it("rejects a value that cannot be parsed", () => {
    expect(resolveCloudReturnTarget("/%", "not-a-url")).toBeNull();
  });

  it("normalizes a bare slash to the root", () => {
    expect(resolveCloudReturnTarget("/", ORIGIN)).toBe("/");
  });
});

describe("cloudHandoffTarget", () => {
  it("falls back to the root without a target", () => {
    expect(cloudHandoffTarget(null, ORIGIN)).toBe("/");
  });

  it("falls back to the root for an unsafe target", () => {
    expect(cloudHandoffTarget("https://evil.example/x", ORIGIN)).toBe("/");
  });

  it("never loops back onto the login page", () => {
    expect(cloudHandoffTarget("/login", ORIGIN)).toBe("/");
    expect(cloudHandoffTarget("/login?next_path=%2Fdocs", ORIGIN)).toBe("/");
    expect(cloudHandoffTarget("/login/", ORIGIN)).toBe("/");
  });

  it("never loops onto the legacy sign-in aliases", () => {
    expect(cloudHandoffTarget("/sign-in", ORIGIN)).toBe("/");
    expect(cloudHandoffTarget("/signin", ORIGIN)).toBe("/");
  });

  it("keeps a deep target with its query", () => {
    const target = "/cloud-ws/projects/abc/issues?peek=activity&tab=comments";
    expect(cloudHandoffTarget(target, ORIGIN)).toBe(target);
  });
});

describe("shouldAttemptSilentSignIn", () => {
  it("resumes with a pending target, no error and no marker", () => {
    expect(shouldAttemptSilentSignIn({ pendingTarget: "/docs", error: null, explicitlyLoggedOut: false })).toBe(true);
  });

  it("stays on the card without a pending target", () => {
    expect(shouldAttemptSilentSignIn({ pendingTarget: null, error: null, explicitlyLoggedOut: false })).toBe(false);
  });

  it("stays on the card when an error landed", () => {
    expect(shouldAttemptSilentSignIn({ pendingTarget: "/docs", error: "oidc_down", explicitlyLoggedOut: false })).toBe(
      false
    );
  });

  it("stays on the card after an explicit logout", () => {
    expect(shouldAttemptSilentSignIn({ pendingTarget: "/docs", error: null, explicitlyLoggedOut: true })).toBe(false);
  });
});

describe("explicit-logout marker", () => {
  it("reads the recorded marker", () => {
    expect(readExplicitLogout(memoryStorage({ [EXPLICIT_LOGOUT_KEY]: "1" }))).toBe(true);
    expect(readExplicitLogout(memoryStorage())).toBe(false);
    expect(readExplicitLogout(memoryStorage({ [EXPLICIT_LOGOUT_KEY]: "0" }))).toBe(false);
  });

  it("marks and clears the tab marker", () => {
    const storage = memoryStorage();
    markExplicitLogout(storage);
    expect(storage.entries.get(EXPLICIT_LOGOUT_KEY)).toBe("1");
    clearExplicitLogout(storage);
    expect(storage.entries.has(EXPLICIT_LOGOUT_KEY)).toBe(false);
  });

  it("swallows storage failures", () => {
    const broken = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
      removeItem: () => {
        throw new Error("denied");
      },
    };
    expect(readExplicitLogout(broken)).toBe(false);
    expect(() => markExplicitLogout(broken)).not.toThrow();
    expect(() => clearExplicitLogout(broken)).not.toThrow();
  });

  it("treats missing storage as no marker", () => {
    expect(readExplicitLogout(null)).toBe(false);
    expect(() => markExplicitLogout(null)).not.toThrow();
    expect(() => clearExplicitLogout(null)).not.toThrow();
  });
});

describe("cloudSignInUrl", () => {
  it("builds the handoff with the target and no same-origin return origin", () => {
    const url = new URL(cloudSignInUrl("https://app.test", ORIGIN, "/docs"));
    expect(url.pathname).toBe(CLOUD_SIGN_IN_HREF);
    expect(url.searchParams.get("return_to")).toBe("/docs");
    expect(url.searchParams.has("return_origin")).toBe(false);
  });

  it("sends the browser origin on split-origin setups", () => {
    const url = new URL(cloudSignInUrl("https://api.test", ORIGIN, "/docs"));
    expect(url.searchParams.get("return_origin")).toBe(ORIGIN);
  });

  it("never carries a fragment", () => {
    expect(cloudSignInUrl("https://app.test", ORIGIN, "/docs#token")).not.toContain("#");
  });
});

describe("module wiring", () => {
  it("keeps the provider handoff path stable", () => {
    expect(CLOUD_SIGN_IN_HREF).toBe("/api/auth/sign-in/");
  });

  it("uses the shared marker token", () => {
    expect(EXPLICIT_LOGOUT_KEY).toBe("pidash:explicitly-logged-out");
  });
});

describe("storage probing", () => {
  it("recovers when sessionStorage access throws", async () => {
    vi.resetModules();
    vi.stubGlobal("sessionStorage", undefined);
    try {
      const module = await import("./cloudLogin.js");
      expect(module.readExplicitLogout(null)).toBe(false);
    } finally {
      vi.unstubAllGlobals();
      vi.resetModules();
    }
  });
});
