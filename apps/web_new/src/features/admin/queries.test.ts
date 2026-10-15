// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import {
  adminCsrfQueryOptions,
  adminKeys,
  adminQueryDefaults,
  adminSessionQueryOptions,
  cloudAuthProbeQueryOptions,
  instanceAdminsQueryOptions,
  instanceBootstrapQueryOptions,
  instanceConfigurationsQueryOptions,
  mergeConfigurationRows,
} from "./queries.js";
import * as adminQueries from "./queries.js";

/** The no-live-update shape every admin query factory must resolve. */
interface AdminPinnedQueryOptions {
  staleTime: unknown;
  refetchOnWindowFocus: unknown;
  refetchOnReconnect: unknown;
  refetchInterval: unknown;
}

type AdminQueryFactory = (client: never) => AdminPinnedQueryOptions;

function isAdminQueryFactory(entry: [string, unknown]): entry is [string, AdminQueryFactory] {
  return entry[0].endsWith("QueryOptions") && typeof entry[1] === "function";
}

describe("admin keys", () => {
  it("roots the admin subtree outside the member roots", () => {
    expect(adminKeys.root).toEqual(["admin"]);
    expect(adminKeys.bootstrap).toEqual(["admin", "instance", "bootstrap"]);
    expect(adminKeys.admins).toEqual(["admin", "instance", "admins"]);
    expect(adminKeys.csrf).toEqual(["admin", "csrf"]);
    expect(adminKeys.session).toEqual(["admin", "session"]);
    expect(adminKeys.cloudProbe).toEqual(["admin", "cloud", "probe"]);
    expect(adminKeys.configurations).toEqual(["admin", "configurations"]);
  });
});

describe("instance bootstrap query", () => {
  it("fetches the bootstrap through the client exactly once", async () => {
    const bootstrap = { config: {}, instance: { is_setup_done: true } };
    const getJson = vi.fn(async () => bootstrap);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceBootstrapQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.bootstrap);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(bootstrap);
    expect(getJson).toHaveBeenCalledWith("/api/instances/");
  });

  it("never retries, refetches, or goes stale (ADM-001 single fetch)", () => {
    const options = instanceBootstrapQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("instance admins query", () => {
  it("lists the admins through the client exactly once", async () => {
    const admins = [{ user_detail: { email: "ada@example.com" } }];
    const getJson = vi.fn(async () => admins);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceAdminsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.admins);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(admins);
    expect(getJson).toHaveBeenCalledWith("/api/instances/admins/");
  });

  it("never retries, refetches, or goes stale (ADM-014 single read)", () => {
    const options = instanceAdminsQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("admin session query", () => {
  const adminMe = {
    id: "22222222-2222-4222-8222-222222222222",
    avatar: "https://cdn.test/avatar.png",
    avatar_url: null,
    cover_image: null,
    date_joined: "2026-01-01T00:00:00Z",
    display_name: "Admin",
    email: "admin@example.com",
    first_name: "Admin",
    last_name: "User",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    user_timezone: "UTC",
    username: "admin",
    is_password_autoset: false,
  };

  it("reads the admin check through the client", async () => {
    const getJson = vi.fn(async () => adminMe);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = adminSessionQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.session);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ kind: "admin", admin: adminMe });
    expect(getJson).toHaveBeenCalledWith("/api/instances/admins/me/");
  });

  it("never retries, refetches, or goes stale (ADM-009 single check)", () => {
    const options = adminSessionQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("instance configurations query", () => {
  const row = (key: string, value: string | null) => ({
    id: "33333333-3333-4333-8333-333333333333",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    key,
    value,
    category: "AUTHENTICATION",
    is_encrypted: false,
    source: "db" as const,
    is_managed: false,
  });

  it("reads the key map through the client", async () => {
    const rows = [row("ENABLE_SIGNUP", "1")];
    const getJson = vi.fn(async () => rows);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceConfigurationsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.configurations);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(rows);
    expect(getJson).toHaveBeenCalledWith("/api/instances/configurations/");
  });

  it("never retries, refetches, or goes stale (shared single fetch)", () => {
    const options = instanceConfigurationsQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });

  it("merges answered rows over the cached map by key", () => {
    const current = [row("ENABLE_SIGNUP", "1"), row("ENABLE_MAGIC_LINK_LOGIN", "0")];
    const updated = [{ ...row("ENABLE_MAGIC_LINK_LOGIN", "1"), updated_at: "2026-02-02T00:00:00Z" }];
    expect(mergeConfigurationRows(current, updated)).toEqual([
      row("ENABLE_SIGNUP", "1"),
      { ...row("ENABLE_MAGIC_LINK_LOGIN", "1"), updated_at: "2026-02-02T00:00:00Z" },
    ]);
  });

  it("starts an empty cache from the answer alone", () => {
    const updated = [row("ENABLE_SIGNUP", "0")];
    expect(mergeConfigurationRows(undefined, updated)).toEqual(updated);
  });
});

describe("admin CSRF query", () => {
  it("fetches the forgery token for the native POSTs", async () => {
    const token = { csrf_token: "token-1" };
    const getJson = vi.fn(async () => token);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = adminCsrfQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.csrf);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(token);
    expect(getJson).toHaveBeenCalledWith("/auth/get-csrf-token/", { csrf: false });
  });

  it("holds the token for the page lifetime", () => {
    const options = adminCsrfQueryOptions({} as never);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
    expect(options.refetchOnWindowFocus).toBe(false);
  });
});

describe("cloud auth probe query", () => {
  const probeUser = {
    id: "user-1",
    email: "user@example.com",
    display_name: "User",
    first_name: "User",
    last_name: "Person",
    avatar: "https://cdn.test/avatar.png",
  };

  it("reads the probe through the client", async () => {
    const getJson = vi.fn(async () => probeUser);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = cloudAuthProbeQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.cloudProbe);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ kind: "authenticated", user: probeUser });
    expect(getJson).toHaveBeenCalledWith("/api/auth/me/");
  });

  it("never retries, refetches, or goes stale (ADM-079 one shot)", () => {
    const options = cloudAuthProbeQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("shared admin query defaults", () => {
  it("pins a cache that never polls and never refetches behind the page", () => {
    expect(adminQueryDefaults.staleTime).toBe(Number.POSITIVE_INFINITY);
    expect(adminQueryDefaults.refetchOnWindowFocus).toBe(false);
    expect(adminQueryDefaults.refetchOnReconnect).toBe(false);
    expect(adminQueryDefaults.refetchInterval).toBe(false);
  });

  it("covers every admin query factory, present and future (ADM-069)", () => {
    // Factories take the client first, so a bare client resolves the
    // shape even for factories whose later params build the key. A new
    // factory that needs more than the client updates this call.
    const factories = (Object.entries(adminQueries) as [string, unknown][]).filter(isAdminQueryFactory);
    expect(factories.length).toBeGreaterThan(0);
    for (const [name, factory] of factories) {
      const options = factory({} as never);
      expect(options.staleTime, `${name} never goes stale`).toBe(Number.POSITIVE_INFINITY);
      expect(options.refetchOnWindowFocus, `${name} never refetches on focus`).toBe(false);
      expect(options.refetchOnReconnect, `${name} never refetches on reconnect`).toBe(false);
      expect(options.refetchInterval ?? false, `${name} never polls`).toBe(false);
    }
  });
});
