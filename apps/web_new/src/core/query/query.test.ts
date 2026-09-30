// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { createQueryClient, LIST_STALE_TIME_MS, REFERENCE_STALE_TIME_MS, workspaceScope } from "./client.js";
import {
  applyOptimisticUpdate,
  invalidateWorkspace,
  removeWorkspace,
  rollbackSnapshots,
  snapshotQueries,
} from "./optimistic.js";
import { createCachePersistor, isReferenceQueryKey, persistReferenceOnly, QUERY_CACHE_VERSION } from "./persistence.js";
import { createMemoryStore } from "../platform/storage.js";
import { createRealtimeHub } from "./realtime.js";

describe("query client defaults", () => {
  it("uses list freshness and refetch-on-focus with no mutation retries", () => {
    const client = createQueryClient();
    expect(client.getDefaultOptions().queries?.staleTime).toBe(LIST_STALE_TIME_MS);
    expect(client.getDefaultOptions().queries?.refetchOnWindowFocus).toBe(true);
    expect(client.getDefaultOptions().mutations?.retry).toBe(false);
    expect(REFERENCE_STALE_TIME_MS).toBeGreaterThan(LIST_STALE_TIME_MS);
  });

  it("scopes every key under the workspace", () => {
    expect(workspaceScope("acme")).toEqual(["ws", "acme"]);
  });
});

describe("optimistic helpers", () => {
  it("patches a cache entry and rolls it back", () => {
    const client = createQueryClient();
    const key = [...workspaceScope("acme"), "detail", "1"];
    client.setQueryData(key, { name: "before" });
    const rollback = applyOptimisticUpdate<{ name: string }>(client, key, (previous) => ({
      name: `${previous?.name}-edited`,
    }));
    expect(client.getQueryData(key)).toEqual({ name: "before-edited" });
    rollback();
    expect(client.getQueryData(key)).toEqual({ name: "before" });
  });

  it("snapshots several keys and restores them all", () => {
    const client = createQueryClient();
    const first = [...workspaceScope("acme"), "list"];
    const second = [...workspaceScope("acme"), "detail", "2"];
    client.setQueryData(first, [1]);
    client.setQueryData(second, { id: "2" });
    const snapshots = snapshotQueries(client, ["ws", "acme"]);
    client.setQueryData(first, [1, 2]);
    client.setQueryData(second, { id: "changed" });
    rollbackSnapshots(client, snapshots);
    expect(client.getQueryData(first)).toEqual([1]);
    expect(client.getQueryData(second)).toEqual({ id: "2" });
  });

  it("invalidates and removes a workspace subtree", async () => {
    const client = createQueryClient();
    const key = [...workspaceScope("acme"), "list"];
    client.setQueryData(key, [], { updatedAt: 0 });
    await invalidateWorkspace(client, "acme");
    expect(client.getQueryState(key)?.isInvalidated).toBe(true);
    removeWorkspace(client, "acme");
    expect(client.getQueryCache().findAll({ queryKey: ["ws", "acme"] })).toEqual([]);
  });
});

describe("cache persistence", () => {
  function seed(client: QueryClient): void {
    client.setQueryData(["ws", "acme", "reference", "states"], [{ id: "s1" }]);
    client.setQueryData(["ws", "acme", "list"], [{ id: "i1" }]);
  }

  it("restores the persisted cache and busts on version mismatch", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    seed(writer);
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryData(["ws", "acme", "reference", "states"])).toEqual([{ id: "s1" }]);
    expect(reader.getQueryData(["ws", "acme", "list"])).toEqual([{ id: "i1" }]);

    const stale = createQueryClient();
    const staleLoader = createCachePersistor(stale, storage, {
      version: QUERY_CACHE_VERSION + 1,
      saveDelayMs: 1,
    });
    await staleLoader.restore();
    staleLoader.stop();
    expect(stale.getQueryData(["ws", "acme", "list"])).toBeUndefined();
    expect(await storage.get("tanstack-query-cache")).toBeNull();
  });

  it("persists reference data only for the web build", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    seed(writer);
    const persistor = createCachePersistor(writer, storage, {
      shouldPersist: persistReferenceOnly,
      saveDelayMs: 1,
    });
    await persistor.save();
    persistor.stop();

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryData(["ws", "acme", "reference", "states"])).toEqual([{ id: "s1" }]);
    expect(reader.getQueryData(["ws", "acme", "list"])).toBeUndefined();
  });

  it("marks reference keys by convention", () => {
    expect(isReferenceQueryKey(["ws", "acme", "reference", "states"])).toBe(true);
    expect(isReferenceQueryKey(["ws", "acme", "list"])).toBe(false);
  });
});

describe("realtime hub stub", () => {
  it("fans out to topic subscribers until unsubscribed", () => {
    const hub = createRealtimeHub();
    const seen: unknown[] = [];
    const unsubscribe = hub.subscribe("issues", (event) => seen.push(event.payload));
    hub.publish({ topic: "issues", payload: { id: "1" } });
    hub.publish({ topic: "other", payload: { id: "2" } });
    unsubscribe();
    hub.publish({ topic: "issues", payload: { id: "3" } });
    expect(seen).toEqual([{ id: "1" }]);
    expect(vi.isMockFunction(unsubscribe)).toBe(false);
  });
});
