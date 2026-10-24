// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fetch-on-mount reads (ISS-221, reused by MOD-039): the option block
// states all three quiet triggers, and a mounted observer fetches exactly
// once while the window gains focus, the browser reconnects and time
// passes — even when the surrounding client turns those triggers on.
import { QueryClient, QueryClientProvider, focusManager, onlineManager, useQuery } from "@tanstack/react-query";
import { act, render } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { FETCH_ON_MOUNT_FRESHNESS, fetchOnMountOptions } from "./read.js";

function Probe({ fetchFn }: { fetchFn: () => Promise<string> }): React.ReactElement {
  useQuery(
    fetchOnMountOptions({
      queryKey: ["probe"],
      queryFn: fetchFn,
    })
  );
  return <span>probe</span>;
}

function renderProbe(fetchFn: () => Promise<string>): void {
  const client = new QueryClient({
    // Deliberately noisy defaults: the read itself must stay quiet.
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true, refetchOnReconnect: true, staleTime: 0 } },
  });
  render(
    <QueryClientProvider client={client}>
      <Probe fetchFn={fetchFn} />
    </QueryClientProvider>
  );
}

/** Let the fetch settle and flush the query's notification microtasks. */
async function settle(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

afterEach(() => {
  // Hand the managers back to their default window listeners.
  focusManager.setFocused(undefined);
  onlineManager.setOnline(true);
  vi.useRealTimers();
});

/**
 * Let a few stale windows pass, then make the browser lose and regain
 * focus and drop and restore the network. The query managers are driven
 * directly: they are what TanStack Query listens to, while a bare "focus"
 * event, or an "online" event on an already-online manager, is a no-op.
 * Time passes first because a refetch-on-focus only fires on stale data.
 */
async function passFocusReconnectAndTime(): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(LIST_STALE_TIME_MS * 4);
    focusManager.setFocused(false);
    focusManager.setFocused(true);
    onlineManager.setOnline(false);
    onlineManager.setOnline(true);
  });
}

describe("FETCH_ON_MOUNT_FRESHNESS", () => {
  it("states the list freshness and every quiet trigger", () => {
    expect(FETCH_ON_MOUNT_FRESHNESS).toEqual({
      staleTime: LIST_STALE_TIME_MS,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      refetchInterval: false,
    });
    expect(Object.isFrozen(FETCH_ON_MOUNT_FRESHNESS)).toBe(true);
  });
});

describe("fetchOnMountOptions", () => {
  it("carries the caller's query through with the quiet block", () => {
    const queryFn = vi.fn(async () => "value");
    const options = fetchOnMountOptions({ queryKey: ["k"], queryFn });
    expect(options.queryKey).toEqual(["k"]);
    expect(options.queryFn).toBe(queryFn);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
  });

  it("wins over a read that states a louder trigger itself", () => {
    // A read that opts back into focus refetching (the estimates list does)
    // still lands on the quiet block through this helper.
    const options = fetchOnMountOptions({ queryKey: ["k"], staleTime: 0, refetchOnWindowFocus: true });
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
  });

  it("fetches once across a focus, a reconnect and a timer tick", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const fetchFn = vi.fn(async () => "value");
    renderProbe(fetchFn);
    await settle();
    expect(fetchFn).toHaveBeenCalledTimes(1);

    await passFocusReconnectAndTime();
    await settle();
    expect(fetchFn).toHaveBeenCalledTimes(1);
  });

  it("refetches when the owner explicitly invalidates the read", async () => {
    const fetchFn = vi.fn(async () => "value");
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <Probe fetchFn={fetchFn} />
      </QueryClientProvider>
    );
    await settle();
    expect(fetchFn).toHaveBeenCalledTimes(1);

    await act(async () => {
      await client.invalidateQueries({ queryKey: ["probe"] });
    });
    await settle();
    expect(fetchFn).toHaveBeenCalledTimes(2);
  });
});
