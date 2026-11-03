// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { QueryClient } from "@tanstack/react-query";
import { ApiError, type Me, type Profile } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import {
  asString,
  CREATE_WORKSPACE_PATH,
  decidePageGate,
  isOnboardedProfile,
  isSessionRejection,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
  parseReturnPath,
  resolveLanding,
  safeNextPath,
} from "./guards.js";
import { sessionKeys } from "./queries.js";

describe("asString", () => {
  it("keeps non-empty strings", () => {
    expect(asString("/acme")).toBe("/acme");
  });

  it("drops empty and non-string values", () => {
    expect(asString("")).toBeUndefined();
    expect(asString(undefined)).toBeUndefined();
    expect(asString(42)).toBeUndefined();
    expect(asString(null)).toBeUndefined();
  });
});

describe("safeNextPath", () => {
  it("keeps internal return paths", () => {
    expect(safeNextPath("/contract-acme")).toBe("/contract-acme");
    expect(safeNextPath("/a/projects/b/issues?layout=compact")).toBe("/a/projects/b/issues?layout=compact");
  });

  it("drops external, protocol-relative and empty values", () => {
    expect(safeNextPath("https://evil.test/")).toBeUndefined();
    expect(safeNextPath("//evil.test/")).toBeUndefined();
    expect(safeNextPath("")).toBeUndefined();
    expect(safeNextPath(undefined)).toBeUndefined();
    expect(safeNextPath(42)).toBeUndefined();
  });
});

describe("parseReturnPath", () => {
  it("reads a safe value from either spelling", () => {
    expect(parseReturnPath({ next: "/acme" })).toEqual({ next: "/acme" });
    expect(parseReturnPath({ next_path: "/acme" })).toEqual({ next: "/acme" });
  });

  it("prefers the first spelling, then the first safe value", () => {
    expect(parseReturnPath({ next: "/a", next_path: "/b" })).toEqual({ next: "/a" });
    expect(parseReturnPath({ next: "https://evil.test/", next_path: "/b" })).toEqual({ next: "/b" });
  });

  it("forwards a present but unsafe value for the server to sanitize", () => {
    expect(parseReturnPath({ next: "https://evil.test/phish" })).toEqual({
      unsafeNext: "https://evil.test/phish",
    });
    expect(parseReturnPath({ next_path: "https://evil.test/phish" })).toEqual({
      unsafeNext: "https://evil.test/phish",
    });
  });

  it("stays absent without a return path", () => {
    expect(parseReturnPath({})).toEqual({});
    expect(parseReturnPath({ next: "", next_path: 42 })).toEqual({});
  });
});

describe("isOnboardedProfile", () => {
  const allFlags = {
    profile_complete: true,
    workspace_create: true,
    workspace_invite: true,
    workspace_join: true,
  };

  it("accepts the onboarded marker", () => {
    expect(isOnboardedProfile({ is_onboarded: true })).toBe(true);
    expect(isOnboardedProfile({ is_onboarded: true, onboarding_step: {} })).toBe(true);
  });

  it("accepts all four progress flags without the marker (finish crash window)", () => {
    expect(isOnboardedProfile({ is_onboarded: false, onboarding_step: { ...allFlags } })).toBe(true);
  });

  it("rejects partial progress flags", () => {
    const { workspace_join: _dropped, ...threeFlags } = allFlags;
    expect(_dropped).toBe(true);
    expect(isOnboardedProfile({ is_onboarded: false, onboarding_step: threeFlags })).toBe(false);
    expect(
      isOnboardedProfile({
        is_onboarded: false,
        onboarding_step: { ...allFlags, profile_complete: false },
      })
    ).toBe(false);
    expect(isOnboardedProfile({ is_onboarded: false, onboarding_step: {} })).toBe(false);
  });

  it("rejects a missing progress bag", () => {
    expect(isOnboardedProfile({ is_onboarded: false })).toBe(false);
    expect(isOnboardedProfile({ is_onboarded: false, onboarding_step: null })).toBe(false);
    expect(isOnboardedProfile({ is_onboarded: false, onboarding_step: undefined })).toBe(false);
  });

  it("fails closed when the profile cannot be read", () => {
    expect(isOnboardedProfile(null)).toBe(false);
    expect(isOnboardedProfile(undefined)).toBe(false);
  });
});

describe("decidePageGate", () => {
  it("renders public pages for everyone", () => {
    expect(
      decidePageGate({ page: "public", signedIn: false, onboarded: false, hasPassword: false, currentPath: "/x" })
    ).toEqual({ action: "render" });
    expect(
      decidePageGate({ page: "public", signedIn: true, onboarded: true, hasPassword: true, currentPath: "/x" })
    ).toEqual({ action: "render" });
  });

  it("renders signed-out-only pages for visitors", () => {
    expect(
      decidePageGate({
        page: "signed-out-only",
        signedIn: false,
        onboarded: false,
        hasPassword: false,
        currentPath: "/sign-in",
      })
    ).toEqual({ action: "render" });
  });

  it("bounces visitors off every other page type with a return path", () => {
    for (const page of ["onboarding-funnel", "set-password", "signed-in-only"] as const) {
      expect(
        decidePageGate({ page, signedIn: false, onboarded: false, hasPassword: false, currentPath: "/acme/issues" })
      ).toEqual({ action: "sign-in", nextPath: "/acme/issues" });
    }
  });

  it("keeps signed-in members out of signed-out-only pages", () => {
    expect(
      decidePageGate({
        page: "signed-out-only",
        signedIn: true,
        onboarded: true,
        hasPassword: true,
        currentPath: "/sign-up",
      })
    ).toEqual({ action: "landing" });
    expect(
      decidePageGate({
        page: "signed-out-only",
        signedIn: true,
        onboarded: false,
        hasPassword: true,
        currentPath: "/sign-up",
      })
    ).toEqual({ action: "onboarding" });
  });

  it("funnels the onboarding route by completion", () => {
    expect(
      decidePageGate({
        page: "onboarding-funnel",
        signedIn: true,
        onboarded: true,
        hasPassword: true,
        currentPath: "/onboarding",
      })
    ).toEqual({ action: "landing" });
    expect(
      decidePageGate({
        page: "onboarding-funnel",
        signedIn: true,
        onboarded: false,
        hasPassword: false,
        currentPath: "/onboarding",
      })
    ).toEqual({ action: "render" });
  });

  it("gates set-password on holding a secret plus being onboarded", () => {
    expect(
      decidePageGate({
        page: "set-password",
        signedIn: true,
        onboarded: true,
        hasPassword: true,
        currentPath: "/set-password",
      })
    ).toEqual({ action: "landing" });
    // Every other combination renders the form.
    expect(
      decidePageGate({
        page: "set-password",
        signedIn: true,
        onboarded: true,
        hasPassword: false,
        currentPath: "/set-password",
      })
    ).toEqual({ action: "render" });
    expect(
      decidePageGate({
        page: "set-password",
        signedIn: true,
        onboarded: false,
        hasPassword: true,
        currentPath: "/set-password",
      })
    ).toEqual({ action: "render" });
    expect(
      decidePageGate({
        page: "set-password",
        signedIn: true,
        onboarded: false,
        hasPassword: false,
        currentPath: "/set-password",
      })
    ).toEqual({ action: "render" });
  });

  it("funnels signed-in-only pages to onboarding until setup finishes", () => {
    expect(
      decidePageGate({
        page: "signed-in-only",
        signedIn: true,
        onboarded: true,
        hasPassword: true,
        currentPath: "/acme",
      })
    ).toEqual({ action: "render" });
    expect(
      decidePageGate({
        page: "signed-in-only",
        signedIn: true,
        onboarded: false,
        hasPassword: true,
        currentPath: "/acme",
      })
    ).toEqual({ action: "onboarding" });
  });
});

describe("resolveLanding", () => {
  const workspaces = [{ slug: "acme" }, { slug: "globex" }] as never;

  it("honors a safe return path first", () => {
    expect(
      resolveLanding({ nextPath: "/acme/projects/p/issues", lastSlug: "acme", fallbackSlug: "globex", workspaces })
    ).toBe("/acme/projects/p/issues");
  });

  it("ignores unsafe return paths and falls through", () => {
    for (const nextPath of ["https://evil.test/phish", "//evil.test/phish", "", undefined]) {
      expect(resolveLanding({ nextPath, lastSlug: "acme", fallbackSlug: "globex", workspaces })).toBe("/acme");
    }
  });

  it("lands on the last workspace while it is still available", () => {
    expect(resolveLanding({ lastSlug: "globex", fallbackSlug: "acme", workspaces })).toBe("/globex");
  });

  it("falls back when the last workspace is gone", () => {
    expect(resolveLanding({ lastSlug: "vanished", fallbackSlug: "acme", workspaces })).toBe("/acme");
    expect(resolveLanding({ lastSlug: null, fallbackSlug: "acme", workspaces })).toBe("/acme");
  });

  it("offers workspace creation when no workspace applies", () => {
    expect(resolveLanding({ lastSlug: null, fallbackSlug: null, workspaces })).toBe(CREATE_WORKSPACE_PATH);
    expect(resolveLanding({ lastSlug: "vanished", fallbackSlug: "gone", workspaces })).toBe(CREATE_WORKSPACE_PATH);
    expect(resolveLanding({ lastSlug: "acme", fallbackSlug: "globex", workspaces: [] })).toBe(CREATE_WORKSPACE_PATH);
    expect(resolveLanding({ lastSlug: "acme", fallbackSlug: "globex", workspaces: undefined })).toBe(
      CREATE_WORKSPACE_PATH
    );
    expect(CREATE_WORKSPACE_PATH).toBe("/create-workspace");
    expect(ONBOARDING_PATH).toBe("/onboarding");
  });
});

describe("gate loaders", () => {
  const me = { id: "user-1", is_password_autoset: false } as Me;
  const profile = { is_onboarded: true } as Profile;

  function stubQueryClient(values: Record<string, unknown>): QueryClient {
    return {
      ensureQueryData: vi.fn(async (options: { queryKey: readonly unknown[] }) => {
        const key = options.queryKey[1];
        if (typeof key !== "string" || !(key in values)) throw new Error(`no fixture for ${String(key)}`);
        const value = values[key];
        if (value instanceof Error) throw value;
        return value;
      }),
    } as unknown as QueryClient;
  }

  it("loads the user and profile for the gate", async () => {
    const queryClient = stubQueryClient({ me, profile });
    await expect(loadGateInputs(queryClient, {} as never)).resolves.toEqual({ me, profile });
    expect(queryClient.ensureQueryData).toHaveBeenCalledTimes(2);
  });

  it("skips the profile read without a session", async () => {
    for (const status of [401, 403]) {
      const queryClient = stubQueryClient({
        me: new ApiError({ status, code: "http", message: "signed out" }),
      });
      await expect(loadGateInputs(queryClient, {} as never)).resolves.toEqual({ me: null, profile: null });
      expect(queryClient.ensureQueryData).toHaveBeenCalledTimes(1);
    }
  });

  it("nulls the profile whose read is rejected", async () => {
    await expect(
      loadGateInputs(
        stubQueryClient({ me, profile: new ApiError({ status: 401, code: "http", message: "gone" }) }),
        {} as never
      )
    ).resolves.toEqual({ me, profile: null });
  });

  it("re-raises transient gate failures instead of recording them as signed-out", async () => {
    const failure = new ApiError({ status: 0, code: "network", message: "socket hang up" });
    await expect(loadGateInputs(stubQueryClient({ me: failure }), {} as never)).rejects.toBe(failure);
    const serverFailure = new ApiError({ status: 500, code: "http", message: "bad gateway" });
    await expect(loadGateInputs(stubQueryClient({ me: serverFailure }), {} as never)).rejects.toBe(serverFailure);
    await expect(loadGateInputs(stubQueryClient({ me, profile: failure }), {} as never)).rejects.toBe(failure);
  });

  it("grades rejections by response status only", () => {
    expect(isSessionRejection(new ApiError({ status: 401, code: "http", message: "x" }))).toBe(true);
    expect(isSessionRejection(new ApiError({ status: 403, code: "http", message: "x" }))).toBe(true);
    expect(isSessionRejection(new ApiError({ status: 500, code: "http", message: "x" }))).toBe(false);
    expect(isSessionRejection(new ApiError({ status: 0, code: "network", message: "x" }))).toBe(false);
    expect(isSessionRejection(new Error("x"))).toBe(false);
    expect(isSessionRejection(null)).toBe(false);
  });

  it("resolves the landing from settings plus the workspace list", async () => {
    const settings = {
      workspace: { last_workspace_slug: "globex", fallback_workspace_slug: "acme" },
    };
    const queryClient = stubQueryClient({
      settings,
      workspaces: [{ slug: "acme" }, { slug: "globex" }],
    });
    await expect(loadLandingTarget(queryClient, {} as never, "/acme/projects/p/issues")).resolves.toBe(
      "/acme/projects/p/issues"
    );
    await expect(loadLandingTarget(queryClient, {} as never)).resolves.toBe("/globex");
    expect(sessionKeys.settings).toEqual(["session", "settings"]);
  });

  it("returns a safe return path without reading settings or workspaces", async () => {
    const queryClient = stubQueryClient({});
    await expect(loadLandingTarget(queryClient, {} as never, "/acme/issues")).resolves.toBe("/acme/issues");
    expect(queryClient.ensureQueryData).not.toHaveBeenCalled();
  });

  it("falls back to creation when settings or workspaces fail to load", async () => {
    const settings = { workspace: { last_workspace_slug: "acme", fallback_workspace_slug: null } };
    await expect(
      loadLandingTarget(stubQueryClient({ settings: new Error("gone"), workspaces: [] }), {} as never)
    ).resolves.toBe(CREATE_WORKSPACE_PATH);
    await expect(
      loadLandingTarget(stubQueryClient({ settings, workspaces: new Error("gone") }), {} as never)
    ).resolves.toBe(CREATE_WORKSPACE_PATH);
  });
});
