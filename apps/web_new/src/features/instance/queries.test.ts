// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import type { ApiClient, Transport } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { createAppClient } from "../../core/api/client.js";
import { createMemoryStore } from "../../core/platform/storage.js";
import { createCachePersistor } from "../../core/query/persistence.js";
import {
  INSTANCE_STARTUP_RETRY_MAX_MS,
  INSTANCE_STARTUP_RETRY_MS,
  appInstanceQueryOptions,
  instanceAppKeys,
  instanceStartupRecheckDelay,
  persistAppCache,
} from "./queries.js";

describe("appInstanceQueryOptions", () => {
  it("keeps the startup read in its own key and recovers without blocking the screen", () => {
    expect(instanceAppKeys.bootstrap).toEqual(["instance", "app", "bootstrap"]);
    const options = appInstanceQueryOptions({} as ApiClient);
    expect(options.queryKey).toEqual(instanceAppKeys.bootstrap);
    expect(options.retry).toBe(false);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.staleTime).toBe(Number.POSITIVE_INFINITY);
  });

  it("backs off between background rechecks and caps the delay", () => {
    expect(instanceStartupRecheckDelay(1)).toBe(INSTANCE_STARTUP_RETRY_MS);
    expect(instanceStartupRecheckDelay(2)).toBe(INSTANCE_STARTUP_RETRY_MS * 2);
    expect(instanceStartupRecheckDelay(0)).toBe(INSTANCE_STARTUP_RETRY_MS);
    expect(instanceStartupRecheckDelay(20)).toBe(INSTANCE_STARTUP_RETRY_MAX_MS);
  });

  it("rechecks on an interval only while the read is failing", () => {
    const options = appInstanceQueryOptions({} as ApiClient);
    const interval = options.refetchInterval as (query: {
      state: { status: string; errorUpdateCount: number };
    }) => number | false;
    expect(interval({ state: { status: "error", errorUpdateCount: 1 } })).toBe(INSTANCE_STARTUP_RETRY_MS);
    expect(interval({ state: { status: "error", errorUpdateCount: 3 } })).toBe(INSTANCE_STARTUP_RETRY_MS * 4);
    expect(interval({ state: { status: "success", errorUpdateCount: 0 } })).toBe(false);
    expect(interval({ state: { status: "pending", errorUpdateCount: 0 } })).toBe(false);
  });

  it("accepts the unprovisioned envelope as an unfinished setup", async () => {
    const client = createAppClient("https://api.test", jsonTransport({ is_activated: false, is_setup_done: false }));
    const options = appInstanceQueryOptions(client);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ setupDone: false, bootstrap: null });
  });

  it("accepts the oracle's partial not-ready instance object", async () => {
    const client = createAppClient(
      "https://api.test",
      jsonTransport({ config: {}, instance: { is_activated: false, is_setup_done: false } })
    );
    const options = appInstanceQueryOptions(client);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ setupDone: false, bootstrap: null });
  });

  it("validates a running instance against the full contract", async () => {
    const client = createAppClient(
      "https://api.test",
      jsonTransport({ config: {}, instance: { is_setup_done: true } })
    );
    const options = appInstanceQueryOptions(client);
    await expect(options.queryFn?.({} as never)).rejects.toBeTruthy();
  });

  it("excludes only the startup key from the desktop cache", () => {
    expect(persistAppCache(instanceAppKeys.bootstrap)).toBe(false);
    expect(persistAppCache(["ws", "acme", "reference", "states"])).toBe(true);
    expect(persistAppCache(["instance", "other", "bootstrap"])).toBe(true);
  });

  it("does not restore the startup answer into a fresh cache", async () => {
    const storage = createMemoryStore();
    const writer = new QueryClient();
    writer.setQueryData(instanceAppKeys.bootstrap, { setupDone: false, bootstrap: null });
    writer.setQueryData(["ws", "acme", "reference", "states"], [{ id: "s1" }]);
    const saver = createCachePersistor(writer, storage, {
      shouldPersist: persistAppCache,
      saveDelayMs: 1,
    });
    await saver.save();
    saver.stop();

    const reader = new QueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryData(instanceAppKeys.bootstrap)).toBeUndefined();
    expect(reader.getQueryData(["ws", "acme", "reference", "states"])).toEqual([{ id: "s1" }]);
  });
});

function jsonTransport(body: unknown): Transport {
  return (url) =>
    Promise.resolve({
      status: 200,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(body)),
    });
}
