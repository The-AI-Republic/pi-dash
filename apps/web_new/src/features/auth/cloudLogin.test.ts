// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  clearExplicitLogout,
  cloudHandoffTarget,
  cloudSignInUrl,
  CLOUD_SIGN_IN_HREF,
  currentPageTarget,
  DESKTOP_OPEN_FAILED,
  DESKTOP_ORIGIN_MISSING,
  DESKTOP_ORIGIN_UNUSABLE,
  EXPLICIT_LOGOUT_KEY,
  markExplicitLogout,
  parseCloudLoginSearch,
  readExplicitLogout,
  resolveCloudReturnTarget,
  shouldAttemptSilentSignIn,
  startCloudHandoff,
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
      "/a\\evil.example/steal",
      "/\t/evil.example/x",
      "workspace/issues/1",
      "/%2e%2e//evil.example/x",
      "/%2e%2e/%2f/evil.example",
      "/a/%2e%2e//evil.example/x",
      "/%5c%5cevil.example/steal",
    ]) {
      expect(resolveCloudReturnTarget(unsafe, ORIGIN)).toBeNull();
    }
  });

  it("rejects a value the parser normalizes onto an off-origin path", () => {
    expect(resolveCloudReturnTarget("/%zz", ORIGIN)).toBeNull();
    expect(resolveCloudReturnTarget("/a/%2e%2e//evil.example/steal", ORIGIN)).toBeNull();
    expect(resolveCloudReturnTarget("/a/%2e%2e/%2f/evil.example", ORIGIN)).toBeNull();
  });

  it("rejects missing values", () => {
    expect(resolveCloudReturnTarget(null, ORIGIN)).toBeNull();
    expect(resolveCloudReturnTarget(undefined, ORIGIN)).toBeNull();
    expect(resolveCloudReturnTarget("", ORIGIN)).toBeNull();
  });

  it("rejects a value that resolves to another origin", () => {
    expect(resolveCloudReturnTarget("/\\evil.example/steal", ORIGIN)).toBeNull();
  });

  it("rejects a percent-encoded backslash after normalization", () => {
    expect(resolveCloudReturnTarget("/%5c%5cevil.example/steal", ORIGIN)).toBeNull();
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

  it("guards every login spelling the router folds onto an alias", () => {
    // The router decodes escapes and matches case-insensitively, so each of
    // these resolves to the login page or one of its legacy aliases.
    const loopTargets = [
      "/login",
      "/login/",
      "/login////",
      "/%6Cogin",
      "/%6cogin",
      "/%4Cogin",
      "/L%6Fgin",
      "/LoGiN",
      "/LOGIN",
      "/login%2F",
      "/%6Cogin%2F",
      "/%6Cogin%3Fnext_path=%2Fdocs",
      "/%6Cogin%23section",
      "/sign-in",
      "/sign-in/",
      "/SIGN-IN",
      "/%73ign-in",
      "/sign-in%2F",
      "/signin",
      "/signin/",
      "/SIGNIN",
      "/%73ignin",
      "/%73ignin%2F",
      "/%73ignin?next_path=%2Fdocs",
    ];
    for (const target of loopTargets) {
      expect([target, cloudHandoffTarget(target, ORIGIN)]).toEqual([target, "/"]);
    }
  });

  it("keeps an encoded path that only resembles a login alias", () => {
    expect(cloudHandoffTarget("/%64ocs", ORIGIN)).toBe("/%64ocs");
    expect(cloudHandoffTarget("/%6Cogin-help", ORIGIN)).toBe("/%6Cogin-help");
  });

  it("keeps paths that only share a prefix with a login alias", () => {
    expect(cloudHandoffTarget("/login-help", ORIGIN)).toBe("/login-help");
  });

  it("never loops onto the legacy sign-in aliases", () => {
    expect(cloudHandoffTarget("/sign-in", ORIGIN)).toBe("/");
    expect(cloudHandoffTarget("/signin", ORIGIN)).toBe("/");
  });

  it("keeps a deep target with its query", () => {
    const target = "/cloud-ws/projects/abc/issues?peek=activity&tab=comments";
    expect(cloudHandoffTarget(target, ORIGIN)).toBe(target);
  });

  it("normalizes the root itself without dropping it", () => {
    expect(cloudHandoffTarget("/", ORIGIN)).toBe("/");
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

describe("currentPageTarget", () => {
  it("keeps the path plus query and drops the fragment", () => {
    expect(currentPageTarget({ pathname: "/sign-up", search: "?next_path=%2Fdocs" })).toBe(
      "/sign-up?next_path=%2Fdocs"
    );
    expect(currentPageTarget({ pathname: "/", search: "" })).toBe("/");
  });
});

describe("startCloudHandoff", () => {
  function shellOf(
    overrides: {
      bakedOrigin?: string | undefined;
      openSystemBrowser?: (url: string) => Promise<void>;
    } = {}
  ) {
    return {
      bakedOrigin: undefined as string | undefined,
      openSystemBrowser: async (_url: string) => undefined,
      ...overrides,
    };
  }

  it("navigates to the handoff on web builds", async () => {
    const navigate = vi.fn();
    await startCloudHandoff({
      shell: undefined,
      apiBase: "https://api.test",
      browserOrigin: "https://app.test",
      returnTo: "/sign-up?next_path=%2Fdocs",
      navigate,
    });
    expect(navigate).toHaveBeenCalledTimes(1);
    const url = new URL(String(navigate.mock.calls[0]?.[0]));
    expect(`${url.origin}${url.pathname}`).toBe("https://api.test/api/auth/sign-in/");
    expect(url.searchParams.get("return_to")).toBe("/sign-up?next_path=%2Fdocs");
    expect(url.searchParams.get("return_origin")).toBe("https://app.test");
    expect(url.searchParams.has("desktop")).toBe(false);
  });

  it("opens the marked handoff in the system browser inside the shell", async () => {
    const openSystemBrowser = vi.fn(async (_url: string) => undefined);
    const navigate = vi.fn();
    const logged: string[] = [];
    const errors = vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
      logged.push(String(message));
    });
    try {
      await startCloudHandoff({
        shell: shellOf({ bakedOrigin: "https://api.test/", openSystemBrowser }),
        apiBase: "https://api.test",
        browserOrigin: "tauri://localhost",
        returnTo: "/",
        navigate,
      });
    } finally {
      errors.mockRestore();
    }
    expect(navigate).not.toHaveBeenCalled();
    expect(logged).toEqual([]);
    expect(openSystemBrowser).toHaveBeenCalledTimes(1);
    const url = new URL(String(openSystemBrowser.mock.calls[0]?.[0]));
    expect(`${url.origin}${url.pathname}`).toBe("https://api.test/api/auth/sign-in/");
    expect(url.searchParams.get("return_to")).toBe("/");
    expect(url.searchParams.get("desktop")).toBe("1");
    expect(url.searchParams.has("return_origin")).toBe(false);
  });

  it("refuses loudly without a baked origin and never navigates", async () => {
    const openSystemBrowser = vi.fn(async (_url: string) => undefined);
    const navigate = vi.fn();
    const onOpenFailed = vi.fn();
    const logged: string[] = [];
    const errors = vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
      logged.push(String(message));
    });
    try {
      await startCloudHandoff({
        shell: shellOf({ openSystemBrowser }),
        apiBase: "https://api.test",
        browserOrigin: "tauri://localhost",
        returnTo: "/",
        navigate,
        onOpenFailed,
      });
    } finally {
      errors.mockRestore();
    }
    expect(navigate).not.toHaveBeenCalled();
    expect(openSystemBrowser).not.toHaveBeenCalled();
    expect(onOpenFailed).not.toHaveBeenCalled();
    expect(logged).toEqual([DESKTOP_ORIGIN_MISSING]);
    expect(DESKTOP_ORIGIN_MISSING).toContain("VITE_API_BASE_URL");
  });

  it("refuses a malformed baked origin instead of failing unhandled", async () => {
    for (const bakedOrigin of ["::::", "ftp://files.test/api", "pidash://auth/callback"]) {
      const openSystemBrowser = vi.fn(async (_url: string) => undefined);
      const navigate = vi.fn();
      const logged: string[] = [];
      const errors = vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
        logged.push(String(message));
      });
      try {
        await startCloudHandoff({
          shell: shellOf({ bakedOrigin, openSystemBrowser }),
          apiBase: "https://api.test",
          browserOrigin: "tauri://localhost",
          returnTo: "/",
          navigate,
        });
      } finally {
        errors.mockRestore();
      }
      expect(navigate).not.toHaveBeenCalled();
      expect(openSystemBrowser).not.toHaveBeenCalled();
      expect(logged).toEqual([DESKTOP_ORIGIN_UNUSABLE]);
    }
  });

  it("logs a blocked browser open and reports it without any fallback", async () => {
    const failure = new Error("opener blocked");
    const navigate = vi.fn();
    const onOpenFailed = vi.fn();
    const logged: string[] = [];
    const errors = vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
      logged.push(String(message));
    });
    try {
      await startCloudHandoff({
        shell: shellOf({
          bakedOrigin: "https://api.test",
          openSystemBrowser: async () => {
            throw failure;
          },
        }),
        apiBase: "https://api.test",
        browserOrigin: "tauri://localhost",
        returnTo: "/",
        navigate,
        onOpenFailed,
      });
    } finally {
      errors.mockRestore();
    }
    expect(navigate).not.toHaveBeenCalled();
    expect(logged).toEqual([DESKTOP_OPEN_FAILED]);
    expect(onOpenFailed).toHaveBeenCalledTimes(1);
    expect(onOpenFailed).toHaveBeenCalledWith(failure);
  });

  it("logs a blocked browser open even with no failure surface", async () => {
    const navigate = vi.fn();
    const logged: string[] = [];
    const errors = vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
      logged.push(String(message));
    });
    try {
      await startCloudHandoff({
        shell: shellOf({
          bakedOrigin: "https://api.test",
          openSystemBrowser: async () => {
            throw new Error("opener blocked");
          },
        }),
        apiBase: "https://api.test",
        browserOrigin: "tauri://localhost",
        returnTo: "/",
        navigate,
      });
    } finally {
      errors.mockRestore();
    }
    expect(navigate).not.toHaveBeenCalled();
    expect(logged).toEqual([DESKTOP_OPEN_FAILED]);
  });
});
