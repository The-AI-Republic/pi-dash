// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration session hook (parity/specs/pages.md §8.4, PAGE-067): the
// editable surface mirrors one live mount per page — loading while the
// chunk resolves, live once the backend walks, degraded when the chunk
// fails — and connectivity or focus fast-forwards a pending retry.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sync = vi.hoisted(() => ({ reportPageSync: vi.fn() }));
const shell = vi.hoisted(() => ({ notifyError: vi.fn() }));
const contracts = vi.hoisted(() => ({ getPageDescriptionBinary: vi.fn() }));

vi.mock("./pageSync.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./pageSync.js")>()),
  reportPageSync: sync.reportPageSync,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: shell.notifyError,
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  getPageDescriptionBinary: contracts.getPageDescriptionBinary,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { resetRealtimeHub } from "../../../core/query/realtime.js";
import type { PageLiveBackend, PageLiveBackendInput } from "./pageLiveBackend.js";
import {
  fetchPageLiveFallbackBinary,
  usePageLiveSession,
  usePageReadOnlySyncReport,
  type UsePageLiveSessionOptions,
} from "./pageLiveSession.js";
import { resetPageLivePropertyEchoes } from "./pageLivePropertyEvents.js";
import { resetPageRealtimeErrors } from "./pageRealtimeErrors.js";
import { pageKeys } from "../api/keys.js";
import { makePageRow } from "../testing.js";

const PAGE_ID = "323e4567-e89b-12d3-a456-426614174002";
const USER = { id: "user-1", name: "Ada" };

function sessionWrapper(
  client = new QueryClient()
): ({ children }: { children: React.ReactNode }) => React.ReactElement {
  return function SessionWrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

interface FakeHarness {
  backend: PageLiveBackend;
  open: () => void;
  close: (code: number) => void;
  synced: () => void;
  stateless: (payload: string) => void;
  cacheReady: () => void;
  connect: ReturnType<typeof vi.fn>;
  destroyBackend: ReturnType<typeof vi.fn>;
  lengths: Map<string, number>;
}

function fakeHarness(options: { cacheReady?: boolean; lengths?: Map<string, number> } = {}): FakeHarness {
  const opens = new Set<() => void>();
  const closes = new Set<(code: number, reason: string) => void>();
  const synceds = new Set<() => void>();
  const frames = new Set<(payload: string) => void>();
  const readys = new Set<() => void>();
  const ready = options.cacheReady ?? true;
  const lengths = options.lengths ?? new Map<string, number>();
  const connect = vi.fn(async () => undefined);
  const destroyBackend = vi.fn(() => undefined);
  let isReady = ready;
  const backend: PageLiveBackend = {
    doc: {
      handle: { marker: "doc" },
      fragmentLength: (field) => lengths.get(field) ?? 0,
      applyBinary: () => undefined,
      destroy: vi.fn(() => undefined),
    },
    connection: {
      onOpen: (listener) => {
        opens.add(listener);
        return () => {
          opens.delete(listener);
        };
      },
      onClose: (listener) => {
        closes.add(listener);
        return () => {
          closes.delete(listener);
        };
      },
      onSynced: (listener) => {
        synceds.add(listener);
        return () => {
          synceds.delete(listener);
        };
      },
      onStateless: (listener) => {
        frames.add(listener);
        return () => {
          frames.delete(listener);
        };
      },
      sendStateless: () => undefined,
      onAuthFailed: () => () => undefined,
      connect,
      disconnect: vi.fn(() => undefined),
      destroy: vi.fn(() => undefined),
    },
    cache: {
      isReady: () => isReady,
      onReady: (listener) => {
        readys.add(listener);
        return () => {
          readys.delete(listener);
        };
      },
    },
    destroy: destroyBackend,
  };
  return {
    backend,
    open: () => {
      for (const listener of opens) listener();
    },
    close: (code) => {
      for (const listener of closes) listener(code, "");
    },
    synced: () => {
      for (const listener of synceds) listener();
    },
    stateless: (payload) => {
      for (const listener of frames) listener(payload);
    },
    cacheReady: () => {
      isReady = true;
      for (const listener of readys) listener();
    },
    connect,
    destroyBackend,
    lengths,
  };
}

function options(overrides: Partial<UsePageLiveSessionOptions> = {}): UsePageLiveSessionOptions {
  return {
    workspaceSlug: "acme",
    projectId: "project-1",
    pageId: PAGE_ID,
    user: USER,
    enabled: true,
    ...overrides,
  };
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((open, fail) => {
    resolve = open;
    reject = fail;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.useFakeTimers();
  createAppClient("https://api.test", () => new Promise<never>(() => undefined));
  vi.clearAllMocks();
  resetRealtimeHub();
  resetPageLivePropertyEchoes();
  contracts.getPageDescriptionBinary.mockResolvedValue(new Uint8Array([1]));
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  resetAppClient();
  resetPageRealtimeErrors();
  resetRealtimeHub();
  resetPageLivePropertyEchoes();
});

describe("usePageLiveSession", () => {
  it("stays null while disabled or signed out", () => {
    const loadBackend = vi.fn(async () => fakeHarness().backend);
    const disabled = renderHook(() => usePageLiveSession(options({ enabled: false, loadBackend })), {
      wrapper: sessionWrapper(),
    });
    expect(disabled.result.current).toBeNull();
    const signedOut = renderHook(() => usePageLiveSession(options({ user: null, loadBackend })), {
      wrapper: sessionWrapper(),
    });
    expect(signedOut.result.current).toBeNull();
    expect(loadBackend).not.toHaveBeenCalled();
    expect(sync.reportPageSync).not.toHaveBeenCalled();
    disabled.unmount();
    signedOut.unmount();
  });

  it("loads the backend with the socket input and reports the mount beat", async () => {
    const harness = fakeHarness();
    const gate = deferred<PageLiveBackend>();
    const seen: PageLiveBackendInput[] = [];
    const loadBackend = vi.fn((input: PageLiveBackendInput) => {
      seen.push(input);
      return gate.promise;
    });
    const rendered = renderHook(
      () => usePageLiveSession(options({ loadBackend, fetchBinary: async () => new Uint8Array([]) })),
      { wrapper: sessionWrapper() }
    );
    expect(rendered.result.current).toMatchObject({ liveReady: false, showLoader: true, docHandle: null });
    expect(sync.reportPageSync).toHaveBeenCalledWith(PAGE_ID, "syncing");
    await act(async () => {
      gate.resolve(harness.backend);
    });
    expect(seen).toHaveLength(1);
    expect(seen[0]?.url).toBe(
      "wss://api.test/live/collaboration?documentType=project_page&projectId=project-1&workspaceSlug=acme"
    );
    expect(seen[0]?.documentName).toBe(PAGE_ID);
    expect(seen[0]?.cacheName).toBe(`page-live-${PAGE_ID}`);
    expect(JSON.parse(seen[0]?.token ?? "{}")).toMatchObject({ id: "user-1", name: "Ada" });
    expect(seen[0]?.userName).toBe("Ada");
    expect(seen[0]?.userColor).toMatch(/^#[0-9a-f]{6}$/);
    expect(rendered.result.current).toMatchObject({
      stage: { kind: "connecting" },
      docHandle: harness.backend.doc.handle,
      cacheEmpty: true,
    });
    expect(harness.connect).toHaveBeenCalledTimes(1);
    rendered.unmount();
  });

  it("mirrors the session walk into the mount", async () => {
    const harness = fakeHarness();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: async () => harness.backend })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    act(() => {
      harness.open();
    });
    expect(rendered.result.current?.stage).toEqual({ kind: "awaiting-sync" });
    expect(rendered.result.current?.showLoader).toBe(true);
    act(() => {
      harness.synced();
    });
    expect(rendered.result.current).toMatchObject({ stage: { kind: "synced" }, liveReady: true, showLoader: false });
    rendered.unmount();
  });

  it("patches the cached rows when a peer locks the page", async () => {
    const client = new QueryClient();
    client.setQueryData(pageKeys.detail("acme", "project-1", PAGE_ID), makePageRow({ id: PAGE_ID, is_locked: false }));
    const harness = fakeHarness();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: async () => harness.backend })), {
      wrapper: sessionWrapper(client),
    });
    await act(async () => undefined);
    act(() => {
      harness.stateless("locked");
    });
    expect(
      client.getQueryData<ReturnType<typeof makePageRow>>(pageKeys.detail("acme", "project-1", PAGE_ID))?.is_locked
    ).toBe(true);
    act(() => {
      harness.stateless("unlocked");
    });
    expect(
      client.getQueryData<ReturnType<typeof makePageRow>>(pageKeys.detail("acme", "project-1", PAGE_ID))?.is_locked
    ).toBe(false);
    rendered.unmount();
  });

  it("mounts warm caches before the sync lands", async () => {
    const harness = fakeHarness({ cacheReady: false, lengths: new Map([["default", 2]]) });
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: async () => harness.backend })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    expect(rendered.result.current?.showLoader).toBe(true);
    act(() => {
      harness.cacheReady();
    });
    expect(rendered.result.current).toMatchObject({ cacheEmpty: false, liveReady: true, showLoader: false });
    rendered.unmount();
  });

  it("fetches the production fallback binary with no injected reader", async () => {
    const harness = fakeHarness();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: async () => harness.backend })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    await act(async () => {
      harness.close(4401);
      let chain = Promise.resolve();
      for (let index = 0; index < 10; index += 1) chain = chain.then(() => undefined);
      await chain;
    });
    expect(contracts.getPageDescriptionBinary).toHaveBeenCalled();
    expect(rendered.result.current).toMatchObject({
      fallback: "done",
      liveReady: true,
      showLoader: false,
      seedDocument: true,
    });
    rendered.unmount();
  });

  it("degrades to the standalone editor when the chunk fails", async () => {
    const gate = deferred<PageLiveBackend>();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: () => gate.promise })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => {
      gate.reject(new Error("chunk gone"));
    });
    expect(rendered.result.current).toMatchObject({
      docHandle: null,
      liveReady: true,
      showLoader: false,
      fallback: "failed",
    });
    // The fault badge owns the surface and the fallback owns persistence.
    expect(sync.reportPageSync).toHaveBeenCalledWith(PAGE_ID, "lost");
    rendered.unmount();
  });

  it("destroys a backend that resolves after unmount", async () => {
    const harness = fakeHarness();
    const gate = deferred<PageLiveBackend>();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: () => gate.promise })), {
      wrapper: sessionWrapper(),
    });
    rendered.unmount();
    await act(async () => {
      gate.resolve(harness.backend);
    });
    expect(harness.destroyBackend).toHaveBeenCalledTimes(1);
    expect(harness.connect).not.toHaveBeenCalled();
  });

  it("ignores a rejection after unmount", async () => {
    const gate = deferred<PageLiveBackend>();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: () => gate.promise })), {
      wrapper: sessionWrapper(),
    });
    rendered.unmount();
    await act(async () => {
      gate.reject(new Error("too late"));
    });
  });

  it("rebuilds the session when the page changes", async () => {
    const first = fakeHarness();
    const second = fakeHarness();
    const loadBackend = vi.fn(async () => first.backend);
    const rendered = renderHook(({ pageId }) => usePageLiveSession(options({ pageId, loadBackend })), {
      initialProps: { pageId: PAGE_ID },
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    expect(rendered.result.current?.docHandle).toBe(first.backend.doc.handle);
    loadBackend.mockResolvedValue(second.backend);
    rendered.rerender({ pageId: "other-page" });
    expect(rendered.result.current).toMatchObject({ liveReady: false, showLoader: true, docHandle: null });
    await act(async () => undefined);
    expect(rendered.result.current?.docHandle).toBe(second.backend.doc.handle);
    expect(first.destroyBackend).toHaveBeenCalledTimes(1);
    rendered.unmount();
    expect(second.destroyBackend).toHaveBeenCalledTimes(1);
  });

  it("drops the session when disabled or signed out mid-flight", async () => {
    const harness = fakeHarness();
    const rendered = renderHook(
      ({ enabled, user }) => usePageLiveSession(options({ enabled, user, loadBackend: async () => harness.backend })),
      {
        initialProps: { enabled: true, user: USER as UsePageLiveSessionOptions["user"] },
        wrapper: sessionWrapper(),
      }
    );
    await act(async () => undefined);
    expect(rendered.result.current?.docHandle).toBe(harness.backend.doc.handle);
    rendered.rerender({ enabled: false, user: USER });
    expect(rendered.result.current).toBeNull();
    expect(harness.destroyBackend).toHaveBeenCalledTimes(1);
    rendered.unmount();
  });

  it("rebuilds when the user changes", async () => {
    const first = fakeHarness();
    const second = fakeHarness();
    const loadBackend = vi.fn(async () => first.backend);
    const rendered = renderHook(({ user }) => usePageLiveSession(options({ user, loadBackend })), {
      initialProps: { user: USER as UsePageLiveSessionOptions["user"] },
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    loadBackend.mockResolvedValue(second.backend);
    rendered.rerender({ user: { id: "user-2", name: "Bo" } });
    await act(async () => undefined);
    expect(rendered.result.current?.docHandle).toBe(second.backend.doc.handle);
    expect(first.destroyBackend).toHaveBeenCalledTimes(1);
    rendered.unmount();
  });

  it("fast-forwards a pending retry on connectivity or focus", async () => {
    const harness = fakeHarness();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: async () => harness.backend })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    act(() => {
      harness.open();
      harness.synced();
      harness.close(1006);
    });
    expect(rendered.result.current?.stage).toEqual({ kind: "reconnecting", attempt: 1 });
    await act(async () => {
      window.dispatchEvent(new window.Event("online"));
    });
    expect(harness.connect).toHaveBeenCalledTimes(2);
    act(() => {
      harness.close(1006);
    });
    await act(async () => {
      window.dispatchEvent(new window.Event("focus"));
    });
    expect(harness.connect).toHaveBeenCalledTimes(3);
    rendered.unmount();
  });

  it("forces a fresh dial on connectivity even while synced", async () => {
    const harness = fakeHarness();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: async () => harness.backend })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => undefined);
    act(() => {
      harness.open();
      harness.synced();
    });
    expect(rendered.result.current?.stage).toEqual({ kind: "synced" });
    await act(async () => {
      window.dispatchEvent(new window.Event("online"));
    });
    expect(harness.connect).toHaveBeenCalledTimes(2);
    expect(rendered.result.current?.stage).toEqual({ kind: "reconnecting", attempt: 1 });
    rendered.unmount();
  });

  it("nudges only on a visible tab and never before the session", async () => {
    const harness = fakeHarness();
    const gate = deferred<PageLiveBackend>();
    const rendered = renderHook(() => usePageLiveSession(options({ loadBackend: () => gate.promise })), {
      wrapper: sessionWrapper(),
    });
    await act(async () => {
      window.dispatchEvent(new window.Event("online"));
      document.dispatchEvent(new window.Event("visibilitychange"));
    });
    await act(async () => {
      gate.resolve(harness.backend);
    });
    act(() => {
      harness.open();
      harness.synced();
      harness.close(1006);
    });
    expect(harness.connect).toHaveBeenCalledTimes(1);
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    await act(async () => {
      document.dispatchEvent(new window.Event("visibilitychange"));
    });
    expect(harness.connect).toHaveBeenCalledTimes(1);
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    await act(async () => {
      document.dispatchEvent(new window.Event("visibilitychange"));
    });
    expect(harness.connect).toHaveBeenCalledTimes(2);
    rendered.unmount();
  });
});

describe("fetchPageLiveFallbackBinary", () => {
  it("reads the stored binary through the app client", async () => {
    createAppClient("https://api.test", () => new Promise<never>(() => undefined));
    contracts.getPageDescriptionBinary.mockResolvedValue(new Uint8Array([7]));
    await expect(fetchPageLiveFallbackBinary("acme", "project-1", PAGE_ID)).resolves.toEqual(new Uint8Array([7]));
    expect(contracts.getPageDescriptionBinary).toHaveBeenCalledWith(expect.anything(), "acme", "project-1", PAGE_ID);
  });
});

describe("usePageLiveSession defaults", () => {
  class FakeWebSocket {
    static instances: FakeWebSocket[] = [];
    static OPEN = 1;
    url: string;
    readyState = 1;
    sent: Array<Uint8Array | string> = [];
    listeners = new Map<string, Set<(event: unknown) => void>>();

    constructor(url: string) {
      FakeWebSocket.instances.push(this);
      this.url = url;
    }

    addEventListener(type: string, listener: (event: unknown) => void): void {
      let listeners = this.listeners.get(type);
      if (listeners === undefined) {
        listeners = new Set();
        this.listeners.set(type, listeners);
      }
      listeners.add(listener);
    }

    removeEventListener(type: string, listener: (event: unknown) => void): void {
      this.listeners.get(type)?.delete(listener);
    }

    send(payload: Uint8Array | string): void {
      this.sent.push(payload);
    }

    close(): void {
      this.readyState = 3;
    }

    emit(type: string, event: unknown): void {
      for (const listener of this.listeners.get(type) ?? []) listener(event);
    }
  }

  it("drives the real chunk against a scripted socket", async () => {
    vi.useRealTimers();
    try {
      FakeWebSocket.instances = [];
      vi.stubGlobal("WebSocket", FakeWebSocket);
      const rendered = renderHook(() => usePageLiveSession(options()), { wrapper: sessionWrapper() });
      await waitFor(() => expect(rendered.result.current?.docHandle).not.toBeNull(), { timeout: 15000 });
      await waitFor(() => expect(FakeWebSocket.instances.length).toBeGreaterThan(0), { timeout: 15000 });
      const socket = FakeWebSocket.instances[0];
      expect(socket?.url).toContain("/live/collaboration?");
      expect(socket?.url).toContain("documentType=project_page");
      expect(socket?.url).toContain("projectId=project-1");
      expect(socket?.url).toContain("workspaceSlug=acme");
      expect(rendered.result.current?.stage).toEqual({ kind: "connecting" });
      await act(async () => {
        socket?.emit("open", {});
      });
      await waitFor(() => expect(rendered.result.current?.stage).toEqual({ kind: "awaiting-sync" }));
      const bytes = (socket?.sent ?? [])
        .map((payload) => (typeof payload === "string" ? payload : Buffer.from(payload).toString("latin1")))
        .join("\n");
      expect(bytes).toContain('"id":"user-1"');
      expect(bytes).toContain('"name":"Ada"');
      expect(bytes).toContain('"color":"#');
      await act(async () => {
        socket?.emit("close", { code: 1006, reason: "" });
      });
      await waitFor(() => expect(rendered.result.current?.stage).toEqual({ kind: "reconnecting", attempt: 1 }));
      rendered.unmount();
    } finally {
      vi.useFakeTimers();
    }
  });
});

describe("usePageReadOnlySyncReport", () => {
  it("reports converged for a snapshot surface", () => {
    sync.reportPageSync.mockClear();
    const rendered = renderHook(() => usePageReadOnlySyncReport(PAGE_ID, true));
    expect(sync.reportPageSync).toHaveBeenCalledWith(PAGE_ID, "synced");
    rendered.unmount();
  });

  it("reports nothing off the snapshot surface", () => {
    sync.reportPageSync.mockClear();
    const rendered = renderHook(() => usePageReadOnlySyncReport(PAGE_ID, false));
    expect(sync.reportPageSync).not.toHaveBeenCalled();
    rendered.unmount();
  });
});
