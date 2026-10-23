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
  instanceEntryQueryOptions,
  instanceRunnersQueryOptions,
  instanceWorkspacesQueryOptions,
  loopJobDetailQueryOptions,
  loopJobsQueryOptions,
  loopJobTargetsQueryOptions,
  mergeConfigurationRows,
  selectNextWorkspaceCursor,
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
    expect(adminKeys.entryBootstrap).toEqual(["admin", "instance", "entry"]);
    expect(adminKeys.admins).toEqual(["admin", "instance", "admins"]);
    expect(adminKeys.csrf).toEqual(["admin", "csrf"]);
    expect(adminKeys.session).toEqual(["admin", "session"]);
    expect(adminKeys.cloudProbe).toEqual(["admin", "cloud", "probe"]);
    expect(adminKeys.configurations).toEqual(["admin", "configurations"]);
    expect(adminKeys.loopJobs).toEqual(["admin", "loop", "jobs"]);
    expect(adminKeys.loopJobDetail("job-1")).toEqual(["admin", "loop", "job", "job-1"]);
    expect(adminKeys.loopJobTargets("job-1", "")).toEqual(["admin", "loop", "targets", "job-1", ""]);
    expect(adminKeys.loopJobTargets("job-1", "min_role")).toEqual(["admin", "loop", "targets", "job-1", "min_role"]);
    expect(adminKeys.workspaces).toEqual(["admin", "workspaces"]);
    expect(adminKeys.runners("", 1)).toEqual(["admin", "runners", "", 1]);
    expect(adminKeys.runners("manual", 2)).toEqual(["admin", "runners", "manual", 2]);
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

describe("instance entry query", () => {
  it("reads once for the gate and folds the bare setup flags", async () => {
    const getJson = vi.fn(async () => ({ is_activated: false, is_setup_done: false }));
    const options = instanceEntryQueryOptions({ getJson } as never);
    expect(options.queryKey).toEqual(adminKeys.entryBootstrap);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ setupDone: false, bootstrap: null });
    expect(getJson).toHaveBeenCalledWith("/api/instances/");
    expect(options.retry).toBe(false);
    expect(options.refetchInterval).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
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

  it("appends answered rows for unknown keys", () => {
    const current = [row("ENABLE_SIGNUP", "1")];
    expect(mergeConfigurationRows(current, [row("EMAIL_HOST", "new.example.com")])).toEqual([
      row("ENABLE_SIGNUP", "1"),
      row("EMAIL_HOST", "new.example.com"),
    ]);
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

describe("instance runners query", () => {
  it("reads one runners page through the client with the filter and page", async () => {
    const page = { page: 2, total: 51, results: [] };
    const getJson = vi.fn(async () => page);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceRunnersQueryOptions(fakeClient, "manual", 2);
    expect(options.queryKey).toEqual(adminKeys.runners("manual", 2));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(page);
    expect(getJson).toHaveBeenCalledWith("/api/instances/runners/", {
      query: { provisioning: "manual", status: undefined, workspace: undefined, page: 2 },
    });
  });

  it("drops the blank filter from the request", async () => {
    const getJson = vi.fn(async () => ({ page: 1, total: 0, results: [] }));
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceRunnersQueryOptions(fakeClient, "", 1);
    expect(options.queryKey).toEqual(adminKeys.runners("", 1));
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/instances/runners/", {
      query: { provisioning: undefined, status: undefined, workspace: undefined, page: 1 },
    });
  });

  it("never retries, refetches, polls, or goes stale", () => {
    const options = instanceRunnersQueryOptions({} as never, "", 1);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
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

describe("loop jobs query", () => {
  it("reads the job list through the client", async () => {
    const jobs = [{ slug: "probe-job" }];
    const getJson = vi.fn(async () => jobs);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = loopJobsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.loopJobs);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(jobs);
    expect(getJson).toHaveBeenCalledWith("/api/instances/loop/jobs/");
  });

  it("never retries, refetches, or goes stale (spec §0.6 admin shape)", () => {
    const options = loopJobsQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("loop job detail query", () => {
  it("reads the job through the client", async () => {
    const job = { slug: "probe-job", stats: { target_count: 0 } };
    const getJson = vi.fn(async () => job);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = loopJobDetailQueryOptions(fakeClient, "job-1");
    expect(options.queryKey).toEqual(adminKeys.loopJobDetail("job-1"));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(job);
    expect(getJson).toHaveBeenCalledWith("/api/instances/loop/jobs/job-1/");
  });

  it("never retries, refetches, polls, or goes stale", () => {
    const options = loopJobDetailQueryOptions({} as never, "job-1");
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("loop job targets query", () => {
  it("reads the targets through the client with the skip filter", async () => {
    const page = { page: 1, results: [] };
    const getJson = vi.fn(async () => page);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = loopJobTargetsQueryOptions(fakeClient, "job-1", "user_disabled");
    expect(options.queryKey).toEqual(adminKeys.loopJobTargets("job-1", "user_disabled"));
    await expect(options.queryFn?.({} as never)).resolves.toEqual(page);
    expect(getJson).toHaveBeenCalledWith("/api/instances/loop/jobs/job-1/targets/", {
      query: { page: undefined, skip_reason: "user_disabled" },
    });
  });

  it("drops the empty skip filter from the request", async () => {
    const getJson = vi.fn(async () => ({ page: 1, results: [] }));
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = loopJobTargetsQueryOptions(fakeClient, "job-1", "");
    expect(options.queryKey).toEqual(adminKeys.loopJobTargets("job-1", ""));
    await options.queryFn?.({} as never);
    expect(getJson).toHaveBeenCalledWith("/api/instances/loop/jobs/job-1/targets/", {
      query: { page: undefined, skip_reason: undefined },
    });
  });

  it("never retries, refetches, polls, or goes stale", () => {
    const options = loopJobTargetsQueryOptions({} as never, "job-1", "");
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("instance configurations query", () => {
  it("reads the key map through the client", async () => {
    const rows = [{ key: "LLM_MODEL", value: "gpt-4o" }];
    const getJson = vi.fn(async () => rows);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceConfigurationsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.configurations);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(rows);
    expect(getJson).toHaveBeenCalledWith("/api/instances/configurations/");
  });

  it("stays cached with no retry, refetch, or polling", () => {
    const options = instanceConfigurationsQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
    expect("refetchInterval" in options ? options.refetchInterval : undefined).toBeUndefined();
  });
});

describe("instance configurations query", () => {
  it("reads the whole key map through the client", async () => {
    const rows = [{ key: "DISABLE_WORKSPACE_CREATION", value: "0" }];
    const getJson = vi.fn(async () => rows);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceConfigurationsQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.configurations);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(rows);
    expect(getJson).toHaveBeenCalledWith("/api/instances/configurations/");
  });

  it("never retries, refetches, or goes stale (ADM-069 shared fetch)", () => {
    const options = instanceConfigurationsQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("selectNextWorkspaceCursor", () => {
  const page = (nextPageResults: boolean, nextCursor: string) =>
    ({ next_page_results: nextPageResults, next_cursor: nextCursor }) as never;

  it("follows the cursor while further results exist", () => {
    expect(selectNextWorkspaceCursor(page(true, "cursor-2"))).toBe("cursor-2");
  });

  it("ends the list when no further results exist", () => {
    expect(selectNextWorkspaceCursor(page(false, "cursor-2"))).toBe(undefined);
  });

  it("ends the list when the cursor is empty", () => {
    expect(selectNextWorkspaceCursor(page(true, ""))).toBe(undefined);
  });
});

describe("instance workspaces query", () => {
  it("reads the first page without a cursor", async () => {
    const envelope = { results: [], next_page_results: false, next_cursor: "" };
    const getJson = vi.fn(async () => envelope);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceWorkspacesQueryOptions(fakeClient);
    expect(options.queryKey).toEqual(adminKeys.workspaces);
    expect(options.initialPageParam).toBe(undefined);
    await expect(options.queryFn?.({ pageParam: undefined } as never)).resolves.toEqual(envelope);
    expect(getJson).toHaveBeenCalledWith("/api/instances/workspaces/", {
      query: { cursor: undefined },
    });
  });

  it("reads later pages with the cursor", async () => {
    const envelope = { results: [], next_page_results: false, next_cursor: "" };
    const getJson = vi.fn(async () => envelope);
    const fakeClient = { getJson, parse: (_schema: unknown, data: unknown) => data } as never;
    const options = instanceWorkspacesQueryOptions(fakeClient);
    await expect(options.queryFn?.({ pageParam: "cursor-2" } as never)).resolves.toEqual(envelope);
    expect(getJson).toHaveBeenCalledWith("/api/instances/workspaces/", {
      query: { cursor: "cursor-2" },
    });
  });

  it("never retries, refetches, or goes stale (ADM-038 single-shot pages)", () => {
    const options = instanceWorkspacesQueryOptions({} as never);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });
});
