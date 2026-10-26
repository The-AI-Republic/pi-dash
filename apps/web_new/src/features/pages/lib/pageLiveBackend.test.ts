// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration backend (parity/specs/pages.md §8, PAGE-065–071): one
// document, one manually driven socket and the local cache per page,
// behind injectable importers; the document ops also run against the
// real conflict-free library.

import { beforeEach, describe, expect, it, vi } from "vitest";
import * as Y from "yjs";

import {
  createPageLiveBackend,
  pageLiveImporters,
  type PageLiveBackendInput,
  type PageLiveImporters,
} from "./pageLiveBackend.js";

const INPUT: PageLiveBackendInput = {
  url: "wss://app.test/live/collaboration?documentType=project_page&projectId=project-1&workspaceSlug=acme",
  documentName: "page-1",
  token: JSON.stringify({ id: "user-1", name: "Ada", color: "#ba2c54" }),
  userName: "Ada",
  userColor: "#ba2c54",
  cacheName: "page-live-page-1",
};

class FakeEmitter {
  handlers = new Map<string, Set<(payload?: unknown) => void>>();

  on(event: string, listener: (payload?: unknown) => void): void {
    let listeners = this.handlers.get(event);
    if (listeners === undefined) {
      listeners = new Set();
      this.handlers.set(event, listeners);
    }
    listeners.add(listener);
  }

  off(event: string, listener: (payload?: unknown) => void): void {
    this.handlers.get(event)?.delete(listener);
  }

  emit(event: string, payload?: unknown): void {
    for (const listener of this.handlers.get(event) ?? []) listener(payload);
  }
}

class FakeDoc {
  lengths = new Map<string, number>();
  applied: Uint8Array[] = [];
  destroyed = false;

  getXmlFragment(name: string): { length: number } {
    return { length: this.lengths.get(name) ?? 0 };
  }

  destroy(): void {
    this.destroyed = true;
  }
}

class FakeSocket extends FakeEmitter {
  static instances: FakeSocket[] = [];
  config: Record<string, unknown>;
  shouldConnect = true;
  connect = vi.fn(async () => undefined);
  disconnect = vi.fn(() => undefined);
  destroy = vi.fn(() => undefined);

  constructor(config: Record<string, unknown>) {
    super();
    FakeSocket.instances.push(this);
    this.config = config;
  }
}

class FakeProvider extends FakeEmitter {
  static instances: FakeProvider[] = [];
  config: Record<string, unknown>;
  awareness: Record<string, unknown> = {};
  destroy = vi.fn(() => undefined);

  constructor(config: Record<string, unknown>) {
    super();
    FakeProvider.instances.push(this);
    this.config = config;
  }

  setAwarenessField(key: string, value: unknown): void {
    this.awareness[key] = value;
  }
}

class FakePersistence extends FakeEmitter {
  static instances: FakePersistence[] = [];
  name: string;
  doc: unknown;
  destroy = vi.fn(() => undefined);

  constructor(name: string, doc: unknown) {
    super();
    FakePersistence.instances.push(this);
    this.name = name;
    this.doc = doc;
  }
}

function stubImporters(overrides: Partial<PageLiveImporters> = {}): PageLiveImporters {
  return {
    yjs: async () => ({
      Doc: FakeDoc,
      applyUpdate: (doc: FakeDoc, update: Uint8Array) => {
        doc.applied.push(update);
      },
    }),
    provider: async () => ({
      HocuspocusProviderWebsocket: FakeSocket,
      HocuspocusProvider: FakeProvider,
    }),
    indexeddb: async () => ({ IndexeddbPersistence: FakePersistence }),
    ...overrides,
  };
}

beforeEach(() => {
  FakeSocket.instances = [];
  FakeProvider.instances = [];
  FakePersistence.instances = [];
  vi.unstubAllGlobals();
});

describe("createPageLiveBackend", () => {
  it("opens the document, the single-shot socket and the cache", async () => {
    vi.stubGlobal("indexedDB", {});
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const socket = FakeSocket.instances[0];
    const provider = FakeProvider.instances[0];
    const persistence = FakePersistence.instances[0];
    expect(socket?.config).toMatchObject({ url: INPUT.url, connect: false, maxAttempts: 1 });
    expect(provider?.config).toMatchObject({ name: INPUT.documentName, token: INPUT.token });
    expect(provider?.config["document"]).toBe(backend.doc.handle);
    expect(provider?.config["websocketProvider"]).toBe(socket);
    expect(provider?.awareness).toEqual({ user: { name: "Ada", color: "#ba2c54" } });
    expect(persistence?.name).toBe(INPUT.cacheName);
    expect(persistence?.doc).toBe(backend.doc.handle);
  });

  it("exposes the default chunk importers", () => {
    expect(typeof pageLiveImporters.yjs).toBe("function");
    expect(typeof pageLiveImporters.provider).toBe("function");
    expect(typeof pageLiveImporters.indexeddb).toBe("function");
  });

  it("loads the persistence chunk through the default importer", async () => {
    const indexeddb = (await pageLiveImporters.indexeddb()) as { IndexeddbPersistence: unknown };
    expect(indexeddb.IndexeddbPersistence).toBeDefined();
  });

  it("forwards socket opens and closes, suppressing the socket retry", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const socket = FakeSocket.instances[0];
    const opened = vi.fn();
    const closed = vi.fn();
    const stopOpen = backend.connection.onOpen(opened);
    const stopClose = backend.connection.onClose(closed);
    socket?.emit("open");
    expect(opened).toHaveBeenCalledTimes(1);
    socket?.emit("disconnect", { event: { code: 1006, reason: "" } });
    expect(socket?.shouldConnect).toBe(false);
    expect(closed).toHaveBeenCalledWith(1006, "");
    stopOpen();
    stopClose();
    socket?.emit("open");
    socket?.emit("disconnect", { event: { code: 1006, reason: "" } });
    expect(opened).toHaveBeenCalledTimes(1);
    expect(closed).toHaveBeenCalledTimes(1);
  });

  it("coerces a malformed close into an abnormal one", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const socket = FakeSocket.instances[0];
    const closed = vi.fn();
    backend.connection.onClose(closed);
    socket?.emit("disconnect", undefined);
    socket?.emit("disconnect", { event: { code: "x", reason: 7 } });
    expect(closed).toHaveBeenNthCalledWith(1, 1006, "");
    expect(closed).toHaveBeenNthCalledWith(2, 1006, "");
  });

  it("forwards only confirmed syncs", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const provider = FakeProvider.instances[0];
    const synced = vi.fn();
    const stop = backend.connection.onSynced(synced);
    provider?.emit("synced", { state: false });
    provider?.emit("synced", undefined);
    expect(synced).not.toHaveBeenCalled();
    provider?.emit("synced", { state: true });
    expect(synced).toHaveBeenCalledTimes(1);
    stop();
    provider?.emit("synced", { state: true });
    expect(synced).toHaveBeenCalledTimes(1);
  });

  it("forwards rejected handshakes", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const provider = FakeProvider.instances[0];
    const authFailed = vi.fn();
    const stop = backend.connection.onAuthFailed(authFailed);
    provider?.emit("authenticationFailed", { reason: "permission-denied" });
    expect(authFailed).toHaveBeenCalledTimes(1);
    stop();
    provider?.emit("authenticationFailed", { reason: "permission-denied" });
    expect(authFailed).toHaveBeenCalledTimes(1);
  });

  it("forwards only string stateless payloads", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const provider = FakeProvider.instances[0];
    const stateless = vi.fn();
    const stop = backend.connection.onStateless(stateless);
    provider?.emit("stateless", { payload: 7 });
    provider?.emit("stateless", undefined);
    expect(stateless).not.toHaveBeenCalled();
    provider?.emit("stateless", { payload: '{"action":"error"}' });
    expect(stateless).toHaveBeenCalledWith('{"action":"error"}');
    stop();
    provider?.emit("stateless", { payload: '{"action":"error"}' });
    expect(stateless).toHaveBeenCalledTimes(1);
  });

  it("drives dials and teardown through the socket", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const socket = FakeSocket.instances[0];
    socket?.emit("open");
    await backend.connection.connect();
    expect(socket?.connect).toHaveBeenCalledTimes(1);
    backend.connection.disconnect();
    expect(socket?.disconnect).toHaveBeenCalledTimes(1);
    backend.connection.destroy();
    expect(socket?.destroy).toHaveBeenCalledTimes(1);
  });

  it("fails a dial that resolves without a live socket", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const socket = FakeSocket.instances[0];
    await expect(backend.connection.connect()).rejects.toThrow("closed before opening");
    socket?.emit("open");
    socket?.emit("close", { event: { code: 1006, reason: "" } });
    await expect(backend.connection.connect()).rejects.toThrow("closed before opening");
    socket?.emit("open");
    await expect(backend.connection.connect()).resolves.toBeUndefined();
  });

  it("reads fragment lengths and merges binary into the document", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const doc = backend.doc.handle as FakeDoc;
    doc.lengths.set("default", 3);
    expect(backend.doc.fragmentLength("default")).toBe(3);
    expect(backend.doc.fragmentLength("title")).toBe(0);
    const bytes = new Uint8Array([1, 2, 3]);
    backend.doc.applyBinary(bytes);
    expect(doc.applied).toEqual([bytes]);
  });

  it("tears down provider, socket and document together", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    const doc = backend.doc.handle as FakeDoc;
    backend.destroy();
    expect(FakeProvider.instances[0]?.destroy).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances[0]?.destroy).toHaveBeenCalledTimes(1);
    expect(doc.destroyed).toBe(true);
  });

  it("resolves the cache once its updates land", async () => {
    vi.stubGlobal("indexedDB", {});
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    expect(backend.cache.isReady()).toBe(false);
    const ready = vi.fn();
    const stop = backend.cache.onReady(ready);
    FakePersistence.instances[0]?.emit("synced");
    expect(backend.cache.isReady()).toBe(true);
    expect(ready).toHaveBeenCalledTimes(1);
    stop();
    FakePersistence.instances[0]?.emit("synced");
    expect(ready).toHaveBeenCalledTimes(1);
  });

  it("proceeds without a cache where no store exists", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters());
    expect(FakePersistence.instances).toHaveLength(0);
    expect(backend.cache.isReady()).toBe(true);
    const ready = vi.fn();
    const stop = backend.cache.onReady(ready);
    stop();
    expect(ready).not.toHaveBeenCalled();
  });

  it("proceeds without a cache when the store fails", async () => {
    vi.stubGlobal("indexedDB", {});
    const backend = await createPageLiveBackend(
      INPUT,
      stubImporters({
        indexeddb: async () => {
          throw new Error("denied");
        },
      })
    );
    expect(backend.cache.isReady()).toBe(true);
  });

  it("merges binary through the real conflict-free document", async () => {
    const backend = await createPageLiveBackend(INPUT, stubImporters({ yjs: () => import("yjs") }));
    expect(backend.doc.fragmentLength("default")).toBe(0);
    const peer = new Y.Doc();
    peer.getXmlFragment("default").insert(0, [new Y.XmlText("hello")]);
    backend.doc.applyBinary(Y.encodeStateAsUpdate(peer));
    expect(backend.doc.fragmentLength("default")).toBe(1);
    expect((backend.doc.handle as Y.Doc).getXmlFragment("default").toString()).toBe("hello");
  });
});
