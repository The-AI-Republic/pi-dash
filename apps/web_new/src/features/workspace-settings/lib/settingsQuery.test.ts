// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fetch policy (WSET-038, spec §38): settings lists read on mount and
// never on a returning focus, a reconnect or a timer.
import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { LIST_STALE_TIME_MS } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "./settingsQuery.js";

describe("WORKSPACE_SETTINGS_QUERY_POLICY", () => {
  it("reads once on mount", () => {
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchOnMount).toBe(true);
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.staleTime).toBe(LIST_STALE_TIME_MS);
  });

  it("never re-reads from focus, reconnects or a poll", () => {
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchOnWindowFocus).toBe(false);
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchOnReconnect).toBe(false);
    expect(WORKSPACE_SETTINGS_QUERY_POLICY.refetchInterval).toBe(false);
  });
});

describe("settings reads under the shared query client", () => {
  /**
   * The slice's contract is the policy, so prove it through the library
   * rather than by re-reading the constants: a fresh client that does
   * refetch on focus (the app's own default) must still refuse a focus
   * or reconnect fetch once the settings policy is applied.
   */
  it("reads once on mount and ignores a returning focus or reconnect", async () => {
    const client = new QueryClient({ defaultOptions: { queries: { refetchOnWindowFocus: true } } });
    const queryFn = vi.fn(async () => "rows");
    const observer = new QueryObserver(client, {
      queryKey: ["ws", "acme", "workspace-settings", "probe"],
      queryFn,
      ...WORKSPACE_SETTINGS_QUERY_POLICY,
      // Stale on purpose: only the focus/reconnect flags may decide then.
      staleTime: 0,
    });
    const unsubscribe = observer.subscribe(() => undefined);
    await vi.waitFor(() => {
      expect(observer.getCurrentResult().data).toBe("rows");
    });
    expect(queryFn).toHaveBeenCalledTimes(1);

    expect(observer.shouldFetchOnWindowFocus()).toBe(false);
    expect(observer.shouldFetchOnReconnect()).toBe(false);
    // The cache entry itself refuses too, so focus/reconnect plumbing
    // cannot reach past the observer.
    const query = client.getQueryCache().find({ queryKey: ["ws", "acme", "workspace-settings", "probe"] });
    expect(query?.observers.every((entry) => !entry.shouldFetchOnWindowFocus())).toBe(true);
    expect(queryFn).toHaveBeenCalledTimes(1);
    unsubscribe();
  });
});
