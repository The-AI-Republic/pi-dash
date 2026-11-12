// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Roster reads (WSET-008/014): both endpoints, their cache keys, and the
// mount-only settings policy.
import { QueryClient, QueryClientProvider, QueryObserver } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { workspaceScope } from "../../../core/query/index.js";
import {
  invitationsQueryOptions,
  rosterKeys,
  rosterQueryOptions,
  usePendingInvitations,
  useWorkspaceRoster,
} from "./api.js";

afterEach(() => {
  resetAppClient();
});

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

describe("roster reads", () => {
  it("keys both lists under the workspace scope", () => {
    expect(rosterKeys.roster("acme")).toEqual([...workspaceScope("acme"), "workspace-settings", "members", "roster"]);
    expect(rosterKeys.invitations("acme")).toEqual([
      ...workspaceScope("acme"),
      "workspace-settings",
      "members",
      "invitations",
    ]);
  });

  it("reads the roster and the pending invitations", async () => {
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      calls.push(url);
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
      }
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify([]),
      };
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(
      () => ({ roster: useWorkspaceRoster("acme"), invitations: usePendingInvitations("acme") }),
      { wrapper: wrapper(queryClient) }
    );
    await waitFor(() => expect(result.current.roster.isSuccess).toBe(true));
    await waitFor(() => expect(result.current.invitations.isSuccess).toBe(true));
    expect(calls.some((url) => url.endsWith("/api/workspaces/acme/members/"))).toBe(true);
    expect(calls.some((url) => url.endsWith("/api/workspaces/acme/invitations/"))).toBe(true);
    // Mount-only reads (§38): both lists follow the settings policy.
    for (const options of [
      rosterQueryOptions(getAppClient(), "acme"),
      invitationsQueryOptions(getAppClient(), "acme"),
    ]) {
      expect(options.refetchOnWindowFocus).toBe(false);
      expect(options.refetchOnReconnect).toBe(false);
      expect(options.refetchInterval).toBe(false);
      // A returning mount still reads: navigation back to the page must
      // show a roster, not an empty cache (§38).
      expect(options.refetchOnMount).toBe(true);
    }
  });

  it("never re-reads from a returning focus or reconnect (§38)", async () => {
    const calls: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      calls.push(url);
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify([]) };
    }) as never);
    // A client that would refetch on focus by default: only the roster
    // policy may refuse the second read.
    const queryClient = new QueryClient({ defaultOptions: { queries: { refetchOnWindowFocus: true } } });
    const reader = new QueryObserver(queryClient, {
      ...rosterQueryOptions(getAppClient(), "acme"),
      // Stale on purpose so freshness cannot be what refuses the read.
      staleTime: 0,
    });
    const unsubscribe = reader.subscribe(() => undefined);
    await waitFor(() => expect(reader.getCurrentResult().isSuccess).toBe(true));
    const reads = (): number => calls.filter((url) => url.endsWith("/api/workspaces/acme/members/")).length;
    expect(reads()).toBe(1);

    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
      window.dispatchEvent(new Event("online"));
    });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(reads()).toBe(1);
    expect(reader.shouldFetchOnWindowFocus()).toBe(false);
    expect(reader.shouldFetchOnReconnect()).toBe(false);
    unsubscribe();
  });
});
