// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
import {
  createCachePersistor,
  isReferenceQueryKey,
  ownerScopedMeta,
  persistReferenceOnly,
  QUERY_CACHE_VERSION,
} from "./persistence.js";
import { createMemoryStore } from "../platform/storage.js";
import { createRealtimeHub } from "./realtime.js";

describe("query client defaults", () => {
  it("uses list freshness with no focus/reconnect refetch and no mutation retries", () => {
    const client = createQueryClient();
    expect(client.getDefaultOptions().queries?.staleTime).toBe(LIST_STALE_TIME_MS);
    // ISS-221: another user's write must not appear without a reload.
    expect(client.getDefaultOptions().queries?.refetchOnWindowFocus).toBe(false);
    expect(client.getDefaultOptions().queries?.refetchOnReconnect).toBe(false);
    expect(client.getDefaultOptions().mutations?.retry).toBe(false);
    expect(REFERENCE_STALE_TIME_MS).toBeGreaterThan(LIST_STALE_TIME_MS);
  });

  it("never refetches an issue query on focus, reconnect or a timer", () => {
    const client = createQueryClient();
    for (const key of [
      ["ws", "acme", "issues", "list"],
      ["ws", "acme", "issues", "detail", "1"],
      ["ws", "acme", "issues", "browse", "ACME-1"],
    ]) {
      client.setQueryData(key, { ok: true });
      const defaults = client.getQueryDefaults(key);
      expect(defaults.refetchOnWindowFocus ?? false).toBe(false);
      expect(defaults.refetchOnReconnect ?? false).toBe(false);
      expect(defaults.refetchInterval ?? false).toBe(false);
    }
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

  it("skips pending queries when saving and keeps the settled ones", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    seed(writer);
    // A query with no data, mid-flight, in the middle of the cache. It used
    // to serialize its promise as `{}` and poison the whole restore.
    const pending = writer.getQueryCache().build(writer, {
      queryKey: ["ws", "acme", "issues", "list", "page"],
      queryFn: () => new Promise<never>(() => undefined),
      retry: false,
    });
    void pending.fetch().catch(() => undefined);
    expect(pending.state.status).toBe("pending");
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const raw = (await storage.get("tanstack-query-cache")) ?? "";
    const stored = JSON.parse(raw) as { state: { queries: Array<{ queryKey: unknown[] }> } };
    expect(stored.state.queries.map((entry) => entry.queryKey)).toEqual([
      ["ws", "acme", "reference", "states"],
      ["ws", "acme", "list"],
    ]);

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryData(["ws", "acme", "list"])).toEqual([{ id: "i1" }]);
    expect(reader.getQueryData(["ws", "acme", "issues", "list", "page"])).toBeUndefined();
  });

  it("keeps an errored query that still holds previously fetched rows", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    seed(writer);
    const listKey = ["ws", "acme", "list"];
    // A failed refetch keeps the last successful data (TanStack keeps the
    // payload and flips the status to "error"). The list still paints those
    // rows and the restore side accepts them, so the save must not drop them.
    const list = writer.getQueryCache().find({ queryKey: listKey });
    list?.setState({ status: "error", error: new Error("offline"), data: [{ id: "i1" }] });
    expect(list?.state.data).toEqual([{ id: "i1" }]);

    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const raw = (await storage.get("tanstack-query-cache")) ?? "";
    const stored = JSON.parse(raw) as {
      state: { queries: Array<{ queryKey: unknown[]; state: { status: string; data?: unknown } }> };
    };
    const savedList = stored.state.queries.find((entry) => entry.queryKey.join("/") === "ws/acme/list");
    expect(savedList?.state.status).toBe("error");
    expect(savedList?.state.data).toEqual([{ id: "i1" }]);

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryData(listKey)).toEqual([{ id: "i1" }]);
    expect(reader.getQueryState(listKey)?.status).toBe("error");
  });

  it("restores the settled entries after a pending entry written by an older build", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    seed(writer);
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const raw = (await storage.get("tanstack-query-cache")) ?? "";
    const payload = JSON.parse(raw) as {
      state: { queries: Array<Record<string, unknown> & { queryKey: unknown[]; state: Record<string, unknown> }> };
    };
    // Old builds persisted pending queries with their promise; JSON keeps
    // only `{}`, and hydrate() would throw on `.then` and drop the rest.
    payload.state.queries.splice(0, 0, {
      dehydratedAt: Date.now(),
      queryKey: ["ws", "acme", "favorites"],
      queryHash: JSON.stringify(["ws", "acme", "favorites"]),
      state: { status: "pending", fetchStatus: "fetching", data: undefined, dataUpdatedAt: 0 },
      promise: {},
    });
    await storage.set("tanstack-query-cache", JSON.stringify(payload));

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await expect(loader.restore()).resolves.toBeUndefined();
    loader.stop();
    expect(reader.getQueryData(["ws", "acme", "reference", "states"])).toEqual([{ id: "s1" }]);
    expect(reader.getQueryData(["ws", "acme", "list"])).toEqual([{ id: "i1" }]);
    expect(reader.getQueryCache().find({ queryKey: ["ws", "acme", "favorites"] })).toBeUndefined();
  });

  it("restores settled error entries, drops pending ones without a promise, and tolerates a missing query list", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    seed(writer);
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const raw = (await storage.get("tanstack-query-cache")) ?? "";
    const payload = JSON.parse(raw) as {
      state: { queries: Array<Record<string, unknown> & { queryKey: unknown[]; state: Record<string, unknown> }> };
    };
    payload.state.queries.splice(0, 0, {
      dehydratedAt: Date.now(),
      queryKey: ["ws", "acme", "detail", "failed"],
      queryHash: JSON.stringify(["ws", "acme", "detail", "failed"]),
      state: { status: "error", fetchStatus: "idle", data: undefined, error: { message: "offline" }, dataUpdatedAt: 0 },
    });
    payload.state.queries.splice(0, 0, {
      dehydratedAt: Date.now(),
      queryKey: ["ws", "acme", "detail", "loading"],
      queryHash: JSON.stringify(["ws", "acme", "detail", "loading"]),
      state: { status: "pending", fetchStatus: "fetching", data: undefined, dataUpdatedAt: 0 },
    });
    await storage.set("tanstack-query-cache", JSON.stringify(payload));

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryData(["ws", "acme", "list"])).toEqual([{ id: "i1" }]);
    expect(reader.getQueryState(["ws", "acme", "detail", "failed"])?.status).toBe("error");
    expect(reader.getQueryCache().find({ queryKey: ["ws", "acme", "detail", "loading"] })).toBeUndefined();

    await storage.set(
      "tanstack-query-cache",
      JSON.stringify({ version: QUERY_CACHE_VERSION, state: { mutations: [] } })
    );
    const empty = createQueryClient();
    const emptyLoader = createCachePersistor(empty, storage, { saveDelayMs: 1 });
    await emptyLoader.restore();
    emptyLoader.stop();
    expect(empty.getQueryCache().getAll()).toEqual([]);
  });

  it("never saves or restores a query marked owner-scoped", async () => {
    // The drafts list is one viewer's rows under the workspace scope, and
    // the desktop build keeps the whole cache on disk between launches.
    // Restoring that list hands the next account on the machine the
    // previous one's drafts (DRAFT-001), so an owner-scoped entry is
    // dropped on both sides of the payload, even in the persist-all build.
    const storage = createMemoryStore();
    const writer = createQueryClient();
    const drafts = writer.getQueryCache().build(writer, {
      queryKey: ["ws", "acme", "issue-drafts", "list"],
      queryFn: () => Promise.resolve({ pages: [{ results: [{ id: "own-draft" }] }], pageParams: [""] }),
      meta: ownerScopedMeta(),
    });
    drafts.setState({
      status: "success",
      fetchStatus: "idle",
      data: { pages: [{ results: [{ id: "own-draft" }] }], pageParams: [""] },
      dataUpdatedAt: Date.now(),
    });
    writer.setQueryData(["ws", "acme", "reference", "states"], [{ id: "s1" }]);
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const raw = (await storage.get("tanstack-query-cache")) ?? "";
    const stored = JSON.parse(raw) as { state: { queries: Array<{ queryKey: unknown[] }> } };
    expect(stored.state.queries.map((entry) => entry.queryKey)).toEqual([["ws", "acme", "reference", "states"]]);

    // A payload written by an older build can still carry the entry.
    stored.state.queries.push({
      dehydratedAt: Date.now(),
      queryKey: ["ws", "acme", "issue-drafts", "list"],
      queryHash: JSON.stringify(["ws", "acme", "issue-drafts", "list"]),
      meta: ownerScopedMeta(),
      state: { status: "success", fetchStatus: "idle", data: { pages: [] }, dataUpdatedAt: 1 },
    });
    await storage.set("tanstack-query-cache", JSON.stringify(stored));

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();
    expect(reader.getQueryCache().find({ queryKey: ["ws", "acme", "issue-drafts", "list"] })).toBeUndefined();
    expect(reader.getQueryData(["ws", "acme", "reference", "states"])).toEqual([{ id: "s1" }]);
  });

  it("marks reference keys by convention", () => {
    expect(isReferenceQueryKey(["ws", "acme", "reference", "states"])).toBe(true);
    expect(isReferenceQueryKey(["ws", "acme", "list"])).toBe(false);
  });

  it("restores a preview that paints at once and still refetches", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    const listKey = ["ws", "acme", "list"];
    writer.setQueryData(listKey, [{ id: "before-archive" }]);
    // Written a moment before launch: inside the list staleTime, so a
    // restore that kept this timestamp would treat the rows as an answer.
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();

    // The saved rows are on screen before anything is fetched.
    expect(reader.getQueryData(listKey)).toEqual([{ id: "before-archive" }]);
    const restored = reader.getQueryCache().find({ queryKey: listKey });
    expect(restored?.state.dataUpdatedAt).toBe(0);
    expect(restored?.isStaleByTime(LIST_STALE_TIME_MS)).toBe(true);
  });

  it("refetches a restored read on its first subscriber and paints the live rows", async () => {
    const storage = createMemoryStore();
    const writer = createQueryClient();
    const listKey = ["ws", "acme", "list"];
    writer.setQueryData(listKey, [{ id: "before-archive" }]);
    const persistor = createCachePersistor(writer, storage, { saveDelayMs: 1 });
    await persistor.save();
    persistor.stop();

    const reader = createQueryClient();
    const loader = createCachePersistor(reader, storage, { saveDelayMs: 1 });
    await loader.restore();
    loader.stop();

    const reads: string[] = [];
    const unsubscribe = reader.getQueryCache().subscribe((event) => {
      if (event.type === "updated" && event.action.type === "success") reads.push("success");
    });
    const result = await reader.fetchQuery({ queryKey: listKey, queryFn: () => Promise.resolve([{ id: "after" }]) });
    unsubscribe();
    expect(reads).toEqual(["success"]);
    expect(result).toEqual([{ id: "after" }]);
    expect(reader.getQueryData(listKey)).toEqual([{ id: "after" }]);
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
