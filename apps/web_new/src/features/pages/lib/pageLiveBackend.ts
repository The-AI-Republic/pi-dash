// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration backend (parity/specs/pages.md §8, PAGE-065–071). One
// conflict-free document per open page, its socket to the live mount,
// and its local cache. The heavy libraries arrive through import() only
// (Stack and dependencies), behind injectable importers so tests drive
// stub doubles while production loads the real chunk.

/** The document field the body editor binds to. */
export const PAGE_LIVE_BODY_FIELD = "default";

/** The document field the title editor binds to. */
export const PAGE_LIVE_TITLE_FIELD = "title";

/** The editor binding: the live document, still opaque outside the chunk. */
export interface PageLiveDocBinding {
  doc: unknown;
}

/** The conflict-free document, plus the few ops the session performs. */
export interface PageLiveDocPort {
  /** The real document, opaque outside the chunk; editors bind to it. */
  readonly handle: unknown;
  /** Top-level children of one named fragment. */
  fragmentLength(field: string): number;
  /** Merge fetched binary state into the document. */
  applyBinary(bytes: Uint8Array): void;
  destroy(): void;
}

/** The live mount socket behind the session's own retry schedule. */
export interface PageLiveConnectionPort {
  onOpen(listener: () => void): () => void;
  onClose(listener: (code: number, reason: string) => void): () => void;
  onSynced(listener: () => void): () => void;
  onStateless(listener: (payload: string) => void): () => void;
  /** One bare stateless payload to the mount (a property wire name). */
  sendStateless(payload: string): void;
  /** A rejected handshake: the mount answers with a message, no close. */
  onAuthFailed(listener: () => void): () => void;
  /** One dial: resolves once open, rejects when the upgrade fails. */
  connect(): Promise<void>;
  disconnect(): void;
  destroy(): void;
}

/** The local cache: ready once its stored updates have landed. */
export interface PageLiveCachePort {
  isReady(): boolean;
  onReady(listener: () => void): () => void;
}

export interface PageLiveBackend {
  readonly doc: PageLiveDocPort;
  readonly connection: PageLiveConnectionPort;
  readonly cache: PageLiveCachePort;
  destroy(): void;
}

export interface PageLiveBackendInput {
  url: string;
  /** The document name on the mount: the page id. */
  documentName: string;
  token: string;
  userName: string;
  userColor: string;
  /** The local cache key: one entry per page. */
  cacheName: string;
}

/** Heavy chunk importers; tests inject stub doubles. */
export interface PageLiveImporters {
  yjs: () => Promise<unknown>;
  provider: () => Promise<unknown>;
  indexeddb: () => Promise<unknown>;
}

export const pageLiveImporters: PageLiveImporters = {
  yjs: () => import("yjs"),
  provider: () => import("@hocuspocus/provider"),
  indexeddb: () => import("y-indexeddb"),
};

interface YjsModule {
  Doc: new () => {
    getXmlFragment(name: string): { length: number };
    destroy(): void;
  };
  applyUpdate: (doc: unknown, update: Uint8Array) => void;
}

interface SocketHandle {
  shouldConnect: boolean;
  connect: () => Promise<unknown>;
  disconnect: () => void;
  destroy: () => void;
  on: (event: string, listener: (payload?: unknown) => void) => void;
  off: (event: string, listener: (payload?: unknown) => void) => void;
}

interface ProviderHandle {
  destroy: () => void;
  setAwarenessField: (key: string, value: unknown) => void;
  sendStateless: (payload: string) => void;
  on: (event: string, listener: (payload?: unknown) => void) => void;
  off: (event: string, listener: (payload?: unknown) => void) => void;
}

interface ProviderModule {
  HocuspocusProviderWebsocket: new (config: {
    url: string;
    connect: boolean;
    maxAttempts: number;
    parameters: Record<string, string>;
  }) => SocketHandle;
  HocuspocusProvider: new (config: {
    websocketProvider: SocketHandle;
    name: string;
    document: unknown;
    token: string;
  }) => ProviderHandle;
}

interface PersistenceHandle {
  on: (event: "synced", listener: () => void) => void;
  off: (event: "synced", listener: () => void) => void;
}

interface IndexeddbModule {
  IndexeddbPersistence: new (name: string, doc: unknown) => PersistenceHandle;
}

function closePayload(payload: unknown): { code: number; reason: string } {
  const event = (payload as { event?: unknown } | null)?.event as { code?: unknown; reason?: unknown } | undefined;
  return {
    code: typeof event?.code === "number" ? event.code : 1006,
    reason: typeof event?.reason === "string" ? event.reason : "",
  };
}

/** A cache that never resolves: used where no store exists. */
function stubCachePort(): PageLiveCachePort {
  return {
    isReady: () => true,
    onReady: () => () => undefined,
  };
}

/**
 * Open one page's backend: a fresh document, a manually driven socket
 * (single-shot dials; the session owns the retry schedule), the user on
 * awareness, and the local cache. Without a store the cache resolves
 * empty immediately and the session proceeds without one.
 */
export async function createPageLiveBackend(
  input: PageLiveBackendInput,
  importers: PageLiveImporters = pageLiveImporters
): Promise<PageLiveBackend> {
  const yjs = (await importers.yjs()) as YjsModule;
  const providerModule = (await importers.provider()) as ProviderModule;
  const doc = new yjs.Doc();
  const socket = new providerModule.HocuspocusProviderWebsocket({
    url: input.url,
    connect: false,
    maxAttempts: 1,
    parameters: {},
  });
  const provider = new providerModule.HocuspocusProvider({
    websocketProvider: socket,
    name: input.documentName,
    document: doc,
    token: input.token,
  });
  provider.setAwarenessField("user", { name: input.userName, color: input.userColor });

  // A dial that closes before its first message resolves instead of
  // rejecting, so the adapter watches the socket itself: a dial that
  // resolves without a live socket underneath has failed.
  let socketOpen = false;
  socket.on("open", () => {
    socketOpen = true;
  });
  socket.on("close", () => {
    socketOpen = false;
  });

  const connection: PageLiveConnectionPort = {
    onOpen: (listener) => {
      const forward = (): void => listener();
      socket.on("open", forward);
      return () => socket.off("open", forward);
    },
    onClose: (listener) => {
      const forward = (payload?: unknown): void => {
        // Suppress the socket's own retry loop: the session schedules
        // every redial, so the close must not trigger one underneath it.
        socket.shouldConnect = false;
        const close = closePayload(payload);
        listener(close.code, close.reason);
      };
      socket.on("disconnect", forward);
      return () => socket.off("disconnect", forward);
    },
    onSynced: (listener) => {
      const forward = (payload?: unknown): void => {
        if ((payload as { state?: unknown } | null)?.state === true) listener();
      };
      provider.on("synced", forward);
      return () => provider.off("synced", forward);
    },
    onStateless: (listener) => {
      const forward = (payload?: unknown): void => {
        const stateless = (payload as { payload?: unknown } | null)?.payload;
        if (typeof stateless === "string") listener(stateless);
      };
      provider.on("stateless", forward);
      return () => provider.off("stateless", forward);
    },
    sendStateless: (payload) => provider.sendStateless(payload),
    onAuthFailed: (listener) => {
      const forward = (): void => listener();
      provider.on("authenticationFailed", forward);
      return () => provider.off("authenticationFailed", forward);
    },
    connect: async () => {
      await socket.connect();
      if (!socketOpen) throw new Error("The collaboration dial closed before opening.");
    },
    disconnect: () => socket.disconnect(),
    destroy: () => socket.destroy(),
  };

  let cache: PageLiveCachePort = stubCachePort();
  if (typeof indexedDB !== "undefined") {
    try {
      const indexeddb = (await importers.indexeddb()) as IndexeddbModule;
      const persistence = new indexeddb.IndexeddbPersistence(input.cacheName, doc);
      let ready = false;
      const listeners = new Set<() => void>();
      const markReady = (): void => {
        ready = true;
        for (const listener of listeners) listener();
      };
      persistence.on("synced", markReady);
      cache = {
        isReady: () => ready,
        onReady: (listener) => {
          listeners.add(listener);
          return () => {
            listeners.delete(listener);
          };
        },
      };
    } catch {
      cache = stubCachePort();
    }
  }

  const docPort: PageLiveDocPort = {
    handle: doc,
    fragmentLength: (field) => doc.getXmlFragment(field).length,
    applyBinary: (bytes) => yjs.applyUpdate(doc, bytes),
    destroy: () => doc.destroy(),
  };
  return {
    doc: docPort,
    connection,
    cache,
    destroy: () => {
      provider.destroy();
      socket.destroy();
      // The persistence listens for the document's own destroy, so one
      // call tears down both the document and its cache.
      docPort.destroy();
    },
  };
}
