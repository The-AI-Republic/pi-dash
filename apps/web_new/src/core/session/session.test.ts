// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import type { TransportResponse } from "@pidash/api-client";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createSessionMiddleware, sessionExpiredMiddleware } from "./middleware.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, selectPermissions } from "./permissions.js";
import {
  decideWorkspaceRouteGate,
  getSessionClient,
  meQueryOptions,
  profileQueryOptions,
  selectWorkspace,
  selectWorkspaceRole,
  sessionKeys,
  setSessionClient,
  workspacesQueryOptions,
} from "./queries.js";
import { registerStoreReset, resetRegisteredStores, signOut, useSessionStore } from "./store.js";

function okResponse(status: number): TransportResponse {
  return {
    status,
    url: "https://example.test/api/x/",
    headers: { get: () => null },
    text: () => Promise.resolve(""),
  };
}

beforeEach(() => {
  useSessionStore.setState({ status: "unknown", returnUrl: null });
});

describe("session store", () => {
  it("moves through authenticated, expired and signed-out", () => {
    const store = useSessionStore.getState();
    store.markAuthenticated();
    expect(useSessionStore.getState().status).toBe("authenticated");
    store.markExpired("/projects/acme/issues");
    expect(useSessionStore.getState()).toMatchObject({ status: "expired", returnUrl: "/projects/acme/issues" });
    store.markSignedOut();
    expect(useSessionStore.getState()).toMatchObject({ status: "signed-out", returnUrl: null });
  });
});

describe("store reset registry", () => {
  it("runs every registered reset and unregisters on demand", () => {
    const calls: string[] = [];
    const unregister = registerStoreReset(() => calls.push("one"));
    registerStoreReset(() => calls.push("two"));
    unregister();
    resetRegisteredStores();
    expect(calls).toEqual(["two"]);
  });

  it("signOut clears registered stores and the query cache", () => {
    const resets: string[] = [];
    const unregister = registerStoreReset(() => resets.push("reset"));
    try {
      const client = new QueryClient();
      client.setQueryData(["ws", "acme", "list"], [{ id: "1" }]);
      useSessionStore.getState().markAuthenticated();
      signOut(client);
      expect(resets).toEqual(["reset"]);
      expect(client.getQueryCache().getAll()).toEqual([]);
      expect(useSessionStore.getState().status).toBe("signed-out");
    } finally {
      unregister();
    }
  });
});

describe("session-expired middleware", () => {
  it("expires the session only on 401", async () => {
    const onUnauthorized = vi.fn();
    const middleware = sessionExpiredMiddleware(onUnauthorized);
    const pass = await middleware({ method: "GET", url: "https://example.test/api/x/", headers: {} }, () =>
      Promise.resolve(okResponse(200))
    );
    expect(pass.status).toBe(200);
    await middleware({ method: "GET", url: "https://example.test/api/x/", headers: {} }, () =>
      Promise.resolve(okResponse(403))
    );
    expect(onUnauthorized).not.toHaveBeenCalled();
    await middleware({ method: "GET", url: "https://example.test/api/x/", headers: {} }, () =>
      Promise.resolve(okResponse(401))
    );
    expect(onUnauthorized).toHaveBeenCalledTimes(1);
  });

  it("wires 401s into the session store", async () => {
    const middleware = createSessionMiddleware();
    await middleware({ method: "GET", url: "https://example.test/api/x/", headers: {} }, () =>
      Promise.resolve(okResponse(401))
    );
    expect(useSessionStore.getState().status).toBe("expired");
  });
});

describe("permissions", () => {
  it("matches the backend role numbers", () => {
    expect(ROLE_ADMIN).toBe(20);
    expect(ROLE_MEMBER).toBe(15);
    expect(ROLE_GUEST).toBe(5);
  });

  it("derives the coarse permission set from the role", () => {
    expect(selectPermissions(null)).toEqual({ role: null, isMember: false, canEdit: false, isAdmin: false });
    expect(selectPermissions(ROLE_GUEST)).toMatchObject({ isMember: true, canEdit: false, isAdmin: false });
    expect(selectPermissions(ROLE_MEMBER)).toMatchObject({ isMember: true, canEdit: true, isAdmin: false });
    expect(selectPermissions(ROLE_ADMIN)).toMatchObject({ isMember: true, canEdit: true, isAdmin: true });
  });
});

describe("session queries", () => {
  const workspaces = [
    { id: "w1", name: "Acme", slug: "acme", logo_url: null, total_members: 3, role: 15 },
    { id: "w2", name: "Other", slug: "other", logo_url: null, total_members: 1, role: 5 },
  ];

  it("selects a workspace by slug or id with its role", () => {
    expect(selectWorkspace(workspaces, "acme")?.id).toBe("w1");
    expect(selectWorkspace(workspaces, "w2")?.slug).toBe("other");
    expect(selectWorkspace(workspaces, "missing")).toBeUndefined();
    expect(selectWorkspaceRole(workspaces, "acme")).toBe(15);
    expect(selectWorkspaceRole(workspaces, "missing")).toBeNull();
  });

  it("lets signed-out visitors through the workspace route so the catch-all can render 404", () => {
    expect(decideWorkspaceRouteGate({ signedIn: false, workspaces, slugOrId: "acme" })).toBe("ok");
    expect(decideWorkspaceRouteGate({ signedIn: false, workspaces: [], slugOrId: "missing" })).toBe("ok");
  });

  it("keeps signed-in non-members out of the workspace route", () => {
    expect(decideWorkspaceRouteGate({ signedIn: true, workspaces, slugOrId: "acme" })).toBe("ok");
    expect(decideWorkspaceRouteGate({ signedIn: true, workspaces, slugOrId: "w2" })).toBe("ok");
    expect(decideWorkspaceRouteGate({ signedIn: true, workspaces, slugOrId: "missing" })).toBe("not-found");
    expect(decideWorkspaceRouteGate({ signedIn: true, workspaces: [], slugOrId: "acme" })).toBe("not-found");
    expect(decideWorkspaceRouteGate({ signedIn: true, workspaces: undefined, slugOrId: "acme" })).toBe("not-found");
  });

  it("requires the bootstrap client before building options", () => {
    expect(() => getSessionClient()).toThrow();
    const fakeClient = {
      getJson: () => Promise.resolve({}),
      parse: (_schema: unknown, data: unknown) => data,
    };
    setSessionClient(fakeClient as never);
    expect(getSessionClient()).toBe(fakeClient);
    expect(meQueryOptions(getSessionClient()).queryKey).toEqual(sessionKeys.me);
    expect(workspacesQueryOptions(getSessionClient()).queryKey).toEqual(sessionKeys.workspaces);
    expect(profileQueryOptions(getSessionClient()).queryKey).toEqual(sessionKeys.profile);
  });

  it("reads the profile through its own key", async () => {
    const fakeClient = {
      getJson: () => Promise.resolve({ is_onboarded: true }),
      parse: (_schema: unknown, data: unknown) => data,
    };
    const options = profileQueryOptions(fakeClient as never);
    expect(sessionKeys.profile).toEqual(["session", "profile"]);
    expect(options.queryKey).toEqual(sessionKeys.profile);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ is_onboarded: true });
  });

  it("reads the instance config through its own key", async () => {
    const { instanceConfigQueryOptions, instanceKeys } = await import("./index.js");
    const flags = { is_smtp_configured: true };
    const fakeClient = {
      getJson: () => Promise.resolve({ config: flags }),
      parse: (_schema: unknown, data: unknown) => data,
    };
    const options = instanceConfigQueryOptions(fakeClient as never);
    expect(instanceKeys.config).toEqual(["instance", "config"]);
    expect(options.queryKey).toEqual(instanceKeys.config);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(flags);
  });
});
