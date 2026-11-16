// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Upkeep reads and writes (parity/specs/profile.md §11, PROF-024): the
// switches flip before the request, a refusal restores the exact payload
// and reports, and the server's answer replaces the cache.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Transport, TransportInit } from "@pidash/api-client";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { notifyError } = vi.hoisted(() => ({ notifyError: vi.fn() }));

vi.mock("../../shared/shell/toaster.js", () => ({ notifyError }));

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { upkeepKeys, useSetUpkeepJobEnabled, useSetUpkeepPaused, useUpkeepSettings } from "./api.js";

type Reply = { status: number; body?: unknown };
interface Call {
  method: string;
  url: string;
  body: unknown;
}

const SETTINGS = {
  enabled: true,
  jobs: [
    { slug: "triage", name: "Triage", description: "Sort.", interval_label: "Daily", enabled: true },
    { slug: "digest", name: "Digest", description: "Summarise.", interval_label: "Weekly", enabled: false },
  ],
};

function csrfReply(url: string) {
  return {
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ csrf_token: "t" }),
  };
}

/** A transport that records real calls and can hold a write until released. */
function stubTransport(reply: () => Reply, gate?: Promise<void>) {
  const calls: Call[] = [];
  const transport: Transport = async (url: string, init: TransportInit) => {
    if (url.endsWith("/auth/get-csrf-token/")) return csrfReply(url);
    calls.push({
      method: init.method,
      url,
      body: typeof init.body === "string" ? (JSON.parse(init.body) as unknown) : init.body,
    });
    if (gate !== undefined && init.method !== "GET") await gate;
    const answer = reply();
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === undefined ? "" : JSON.stringify(answer.body)),
    };
  };
  return { transport, calls };
}

function setup(reply: () => Reply, gate?: Promise<void>, seed = true) {
  const { transport, calls } = stubTransport(reply, gate);
  createAppClient("https://api.test", transport);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  if (seed) client.setQueryData(upkeepKeys.settings, SETTINGS);
  function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={client}>{children as never}</QueryClientProvider>;
  }
  return { calls, client, Wrapper };
}

function gate(): { promise: Promise<void>; release: () => void } {
  let release: () => void = () => undefined;
  const promise = new Promise<void>((done) => {
    release = done;
  });
  return { promise, release };
}

beforeEach(() => notifyError.mockReset());
afterEach(() => resetAppClient());

describe("upkeep reads", () => {
  it("reads the member's own switches", async () => {
    const { Wrapper, calls } = setup(() => ({ status: 200, body: SETTINGS }), undefined, false);
    const { result } = renderHook(() => useUpkeepSettings(), { wrapper: Wrapper });
    await waitFor(() => expect(result.current.data).toEqual(SETTINGS));
    expect(calls[0]).toMatchObject({ method: "GET", url: "https://api.test/api/users/me/auto-pm/" });
  });
});

describe("master pause", () => {
  it("flips instantly, then keeps the server's answer", async () => {
    const held = gate();
    const { Wrapper, client, calls } = setup(
      () => ({ status: 200, body: { ...SETTINGS, enabled: false } }),
      held.promise
    );
    const { result } = renderHook(() => useSetUpkeepPaused(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate(true);
    });
    await waitFor(() => expect(client.getQueryData(upkeepKeys.settings)).toEqual({ ...SETTINGS, enabled: false }));
    held.release();
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(client.getQueryData(upkeepKeys.settings)).toEqual({ ...SETTINGS, enabled: false });
    expect(calls[0]).toMatchObject({ method: "PATCH", body: { enabled: false } });
  });

  it("sends the inverse of the paused flag", async () => {
    const { Wrapper, calls } = setup(() => ({ status: 200, body: { ...SETTINGS, enabled: true } }));
    const { result } = renderHook(() => useSetUpkeepPaused(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate(false);
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({
      method: "PATCH",
      url: "https://api.test/api/users/me/auto-pm/",
      body: { enabled: true },
    });
  });

  it("rolls back the exact payload and reports the server's message", async () => {
    const { Wrapper, client } = setup(() => ({ status: 400, body: { error: "paused_by_admin" } }));
    const { result } = renderHook(() => useSetUpkeepPaused(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate(true);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    await waitFor(() => expect(client.getQueryData(upkeepKeys.settings)).toEqual(SETTINGS));
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "paused_by_admin");
  });

  it("falls back to its own line when the refusal carries no message", async () => {
    const { Wrapper } = setup(() => ({ status: 500 }));
    const { result } = renderHook(() => useSetUpkeepPaused(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate(true);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Please try again.");
  });
});

describe("job toggle", () => {
  it("flips one job instantly and patches its slug", async () => {
    const held = gate();
    const { Wrapper, client, calls } = setup(
      () => ({ status: 200, body: { ...SETTINGS, jobs: [{ ...SETTINGS.jobs[0], enabled: false }, SETTINGS.jobs[1]] } }),
      held.promise
    );
    const { result } = renderHook(() => useSetUpkeepJobEnabled(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate({ slug: "triage", enabled: false });
    });
    await waitFor(() =>
      expect((client.getQueryData(upkeepKeys.settings) as typeof SETTINGS).jobs[0]?.enabled).toBe(false)
    );
    expect((client.getQueryData(upkeepKeys.settings) as typeof SETTINGS).jobs[1]?.enabled).toBe(false);
    held.release();
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]).toMatchObject({
      method: "PATCH",
      url: "https://api.test/api/users/me/auto-pm/jobs/triage/",
      body: { enabled: false },
    });
  });

  it("rolls a failed job write back and reports", async () => {
    const { Wrapper, client } = setup(() => ({ status: 404, body: { detail: "No such job." } }));
    const { result } = renderHook(() => useSetUpkeepJobEnabled(), { wrapper: Wrapper });
    act(() => {
      result.current.mutate({ slug: "digest", enabled: true });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    await waitFor(() => expect(client.getQueryData(upkeepKeys.settings)).toEqual(SETTINGS));
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "No such job.");
  });
});
