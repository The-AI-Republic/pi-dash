// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration session (parity/specs/pages.md §8.4, §8.6, PAGE-067,
// PAGE-069): the dial walks connecting, awaiting-sync and synced, drops
// retry on backoff with counted attempts into terminal disconnected,
// frames route to the realtime seams, and a terminal disconnect with an
// empty cache renders the separately fetched binary.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const sync = vi.hoisted(() => ({ reportPageSync: vi.fn() }));
const shell = vi.hoisted(() => ({ notifyError: vi.fn() }));

vi.mock("./pageSync.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./pageSync.js")>()),
  reportPageSync: sync.reportPageSync,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: shell.notifyError,
}));

import {
  createPageLiveSession,
  PAGE_LIVE_LOAD_FAILED_TITLE,
  PAGE_LIVE_SAVE_FAILED_TITLE,
  type PageLiveSession,
} from "./pageLiveSession.js";
import { PAGE_CONTENT_TOO_LARGE, resetPageRealtimeErrors, subscribePageRealtimeErrors } from "./pageRealtimeErrors.js";
import type { PageLiveBackend } from "./pageLiveBackend.js";
import { getRealtimeHub, resetRealtimeHub } from "../../../core/query/realtime.js";
import { PAGES_REALTIME_TOPIC } from "../api/rowMutations.js";
import { resetPageLivePropertyEchoes } from "./pageLivePropertyEvents.js";

const PAGE_ID = "323e4567-e89b-12d3-a456-426614174002";

function flush(): Promise<void> {
  let chain = Promise.resolve();
  for (let index = 0; index < 10; index += 1) chain = chain.then(() => undefined);
  return chain;
}

interface FakeHarness {
  backend: PageLiveBackend;
  open: () => void;
  close: (code: number) => void;
  synced: () => void;
  stateless: (payload: string) => void;
  authFailed: () => void;
  cacheReady: () => void;
  connect: ReturnType<typeof vi.fn>;
  disconnect: ReturnType<typeof vi.fn>;
  destroyBackend: ReturnType<typeof vi.fn>;
  sendStateless: ReturnType<typeof vi.fn<(payload: string) => void>>;
  applied: Uint8Array[];
  lengths: Map<string, number>;
  applyBinary: ReturnType<typeof vi.fn<(bytes: Uint8Array) => void>>;
  fetchBinary: ReturnType<typeof vi.fn<() => Promise<Uint8Array>>>;
}

function fakeHarness(options: { cacheReady?: boolean; lengths?: Map<string, number> } = {}): FakeHarness {
  const opens = new Set<() => void>();
  const closes = new Set<(code: number, reason: string) => void>();
  const synceds = new Set<() => void>();
  const frames = new Set<(payload: string) => void>();
  const authFailures = new Set<() => void>();
  const readys = new Set<() => void>();
  let ready = options.cacheReady ?? true;
  const lengths = options.lengths ?? new Map<string, number>();
  const applied: Uint8Array[] = [];
  const connect = vi.fn(async () => undefined);
  const disconnect = vi.fn(() => undefined);
  const destroyBackend = vi.fn(() => undefined);
  const sendStateless = vi.fn((_payload: string) => undefined);
  const applyBinary = vi.fn((bytes: Uint8Array) => {
    applied.push(bytes);
  });
  const backend: PageLiveBackend = {
    doc: {
      handle: { marker: "doc" },
      fragmentLength: (field) => lengths.get(field) ?? 0,
      applyBinary,
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
      sendStateless,
      onAuthFailed: (listener) => {
        authFailures.add(listener);
        return () => {
          authFailures.delete(listener);
        };
      },
      connect,
      disconnect,
      destroy: vi.fn(() => undefined),
    },
    cache: {
      isReady: () => ready,
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
    authFailed: () => {
      for (const listener of authFailures) listener();
    },
    cacheReady: () => {
      ready = true;
      for (const listener of readys) listener();
    },
    connect,
    disconnect,
    destroyBackend,
    sendStateless,
    applied,
    lengths,
    applyBinary,
    fetchBinary: vi.fn(async () => new Uint8Array([1, 2, 3])),
  };
}

function startSession(harness: FakeHarness, onPropertyEvent?: (event: string) => void): PageLiveSession {
  const session = createPageLiveSession({
    pageId: PAGE_ID,
    backend: harness.backend,
    fetchBinary: harness.fetchBinary,
    ...(onPropertyEvent === undefined ? {} : { onPropertyEvent }),
  });
  session.start();
  return session;
}

function publishProperty(pageId: string, event: unknown): void {
  getRealtimeHub().publish({ topic: PAGES_REALTIME_TOPIC, payload: { pageId, event } });
}

function errorFrame(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    action: "error",
    affectedPages: { currentPage: PAGE_ID },
    data: {
      error_message: "The save was refused.",
      error_type: "store",
      error_code: "page_locked",
      should_disconnect: false,
    },
    ...overrides,
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  resetRealtimeHub();
  resetPageLivePropertyEchoes();
});

afterEach(() => {
  vi.useRealTimers();
  resetPageRealtimeErrors();
  resetRealtimeHub();
  resetPageLivePropertyEchoes();
});

describe("createPageLiveSession", () => {
  it("starts once: a single dial with the transitional report", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    session.start();
    expect(harness.connect).toHaveBeenCalledTimes(1);
    expect(harness.disconnect).toHaveBeenCalledTimes(1);
    expect(session.getStage()).toEqual({ kind: "connecting" });
    expect(sync.reportPageSync).toHaveBeenCalledWith(PAGE_ID, "syncing");
  });

  it("walks open and synced to the hidden badge", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    expect(session.getStage()).toEqual({ kind: "awaiting-sync" });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "syncing");
    harness.synced();
    expect(session.getStage()).toEqual({ kind: "synced" });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "synced");
  });

  it("ignores a repeated open on the same stage", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    const notified = vi.fn();
    session.subscribe(notified);
    harness.open();
    expect(session.getStage()).toEqual({ kind: "awaiting-sync" });
    sync.reportPageSync.mockClear();
    notified.mockClear();
    harness.open();
    expect(session.getStage()).toEqual({ kind: "awaiting-sync" });
    expect(sync.reportPageSync).not.toHaveBeenCalled();
    expect(notified).not.toHaveBeenCalled();
  });

  it("retries a drop on backoff with counted attempts", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    harness.synced();
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "syncing");
    vi.advanceTimersByTime(999);
    expect(harness.connect).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(harness.connect).toHaveBeenCalledTimes(2);
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 2 });
    vi.advanceTimersByTime(2000);
    expect(harness.connect).toHaveBeenCalledTimes(3);
  });

  it("drops a dial the socket refuses", async () => {
    const harness = fakeHarness();
    harness.connect.mockRejectedValueOnce(new Error("refused"));
    const session = startSession(harness);
    await flush();
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
    vi.advanceTimersByTime(1000);
    expect(harness.connect).toHaveBeenCalledTimes(2);
  });

  it("ignores a refused dial once destroyed", async () => {
    const harness = fakeHarness();
    let reject!: (error: unknown) => void;
    harness.connect.mockReturnValueOnce(
      new Promise<undefined>((_, fail) => {
        reject = fail;
      })
    );
    const session = startSession(harness);
    session.destroy();
    reject(new Error("gone"));
    await flush();
    expect(session.getStage()).toEqual({ kind: "connecting" });
    vi.advanceTimersByTime(120_000);
    expect(harness.connect).toHaveBeenCalledTimes(1);
  });

  it("drops an attempt that never opens", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    vi.advanceTimersByTime(10_000);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
    vi.advanceTimersByTime(1000);
    expect(harness.connect).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(10_000);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 2 });
  });

  it("ends auth closes immediately without another dial", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.close(4401);
    expect(session.getStage()).toEqual({ kind: "disconnected", reason: "auth" });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "lost");
    vi.advanceTimersByTime(60_000);
    expect(harness.connect).toHaveBeenCalledTimes(1);
  });

  it("ends a rejected handshake immediately and drops the idling socket", async () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    expect(session.getStage()).toEqual({ kind: "awaiting-sync" });
    harness.authFailed();
    expect(session.getStage()).toEqual({ kind: "disconnected", reason: "auth" });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "lost");
    expect(harness.disconnect).toHaveBeenCalledTimes(2);
    // A repeat and the socket's own late close stay terminal: no redial.
    harness.authFailed();
    harness.close(1006);
    vi.advanceTimersByTime(120_000);
    expect(session.getStage()).toEqual({ kind: "disconnected", reason: "auth" });
    expect(harness.connect).toHaveBeenCalledTimes(1);
    expect(harness.disconnect).toHaveBeenCalledTimes(2);
    await flush();
    expect(harness.fetchBinary).toHaveBeenCalledTimes(1);
    expect(session.getFallback()).toBe("done");
  });

  it("exhausts retries into terminal with the fault report", async () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    for (let attempt = 1; attempt <= 8; attempt += 1) {
      harness.close(1006);
      expect(session.getStage()).toEqual({ kind: "reconnecting", attempt });
      vi.advanceTimersByTime(attempt < 4 ? 1000 * 2 ** (attempt - 1) : 10_000);
    }
    expect(harness.connect).toHaveBeenCalledTimes(9);
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "disconnected", reason: "exhausted" });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "lost");
    vi.advanceTimersByTime(120_000);
    expect(harness.connect).toHaveBeenCalledTimes(9);
    await flush();
    expect(harness.fetchBinary).toHaveBeenCalledTimes(1);
    expect(harness.applied).toHaveLength(1);
    expect(session.getFallback()).toBe("done");
  });

  it("ends a force close immediately without another dial", async () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    harness.synced();
    harness.close(4001);
    expect(session.getStage()).toEqual({ kind: "disconnected", reason: "force-close" });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "lost");
    // The server already unloaded the document: a repeat close stays
    // terminal and no redial re-loads it.
    harness.close(4000);
    vi.advanceTimersByTime(120_000);
    expect(harness.connect).toHaveBeenCalledTimes(1);
    await flush();
    expect(harness.fetchBinary).toHaveBeenCalledTimes(1);
    expect(session.getFallback()).toBe("done");
  });

  it("resets the retry budget on every clean sync", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    harness.synced();
    // Ten recovered drops never accumulate: each sync restarts the count.
    for (let cycle = 0; cycle < 10; cycle += 1) {
      harness.close(1006);
      expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
      vi.advanceTimersByTime(1000);
      harness.open();
      harness.synced();
      expect(session.getStage()).toEqual({ kind: "synced" });
    }
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
    expect(harness.connect).toHaveBeenCalledTimes(11);
  });

  it("resets the retry budget when the socket opens", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    harness.synced();
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
    vi.advanceTimersByTime(1000);
    harness.open();
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
  });

  it("ignores a stale close once terminal", async () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.close(4401);
    await flush();
    harness.close(1006);
    vi.advanceTimersByTime(120_000);
    expect(session.getStage()).toEqual({ kind: "disconnected", reason: "auth" });
    expect(harness.connect).toHaveBeenCalledTimes(1);
    expect(harness.fetchBinary).toHaveBeenCalledTimes(1);
  });

  it("routes error frames to the realtime seam with a notice", () => {
    const harness = fakeHarness();
    const seen: Array<{ code: string; message: string }> = [];
    subscribePageRealtimeErrors((error) => {
      seen.push({ code: error.code, message: error.message });
    });
    startSession(harness);
    harness.stateless(errorFrame());
    expect(seen).toEqual([{ code: "page_locked", message: "The save was refused." }]);
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_LIVE_SAVE_FAILED_TITLE, "The save was refused.");
  });

  it("titles a load failure for the load phase", () => {
    const harness = fakeHarness();
    startSession(harness);
    harness.stateless(errorFrame({ data: { error_message: "The load failed.", error_type: "fetch" } }));
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_LIVE_LOAD_FAILED_TITLE, "The load failed.");
  });

  it("leaves the oversized code to its strip", () => {
    const harness = fakeHarness();
    const seen: string[] = [];
    subscribePageRealtimeErrors((error) => {
      seen.push(error.code);
    });
    startSession(harness);
    harness.stateless(
      errorFrame({ data: { error_message: "Too large.", error_type: "store", error_code: PAGE_CONTENT_TOO_LARGE } })
    );
    expect(seen).toEqual([PAGE_CONTENT_TOO_LARGE]);
    expect(shell.notifyError).not.toHaveBeenCalled();
  });

  it("notices each distinct failure once", () => {
    const harness = fakeHarness();
    startSession(harness);
    harness.stateless(errorFrame());
    harness.stateless(errorFrame());
    expect(shell.notifyError).toHaveBeenCalledTimes(1);
    harness.stateless(errorFrame({ data: { error_message: "Another refusal.", error_type: "store" } }));
    expect(shell.notifyError).toHaveBeenCalledTimes(2);
  });

  it("notices a recurring failure again after a clean sync", () => {
    const harness = fakeHarness();
    startSession(harness);
    harness.stateless(errorFrame());
    expect(shell.notifyError).toHaveBeenCalledTimes(1);
    harness.open();
    harness.synced();
    harness.stateless(errorFrame());
    expect(shell.notifyError).toHaveBeenCalledTimes(2);
  });

  it("ignores malformed frames and force-close notices", () => {
    const harness = fakeHarness();
    const seen: string[] = [];
    subscribePageRealtimeErrors((error) => {
      seen.push(error.code);
    });
    startSession(harness);
    harness.stateless("not json");
    harness.stateless(JSON.stringify({ type: "force_close", reason: "document_too_large", code: 4001 }));
    harness.stateless(errorFrame({ affectedPages: { currentPage: "another-page" } }));
    expect(seen).toEqual([]);
    expect(shell.notifyError).not.toHaveBeenCalled();
  });

  it("reads a warm cache and skips the fallback fetch", async () => {
    const harness = fakeHarness({ cacheReady: false, lengths: new Map([["default", 2]]) });
    const session = startSession(harness);
    expect(session.getCacheEmpty()).toBeNull();
    harness.cacheReady();
    harness.cacheReady();
    expect(session.getCacheEmpty()).toBe(false);
    harness.close(4401);
    await flush();
    expect(harness.fetchBinary).not.toHaveBeenCalled();
    expect(session.getFallback()).toBe("idle");
  });

  it("evaluates a never-ready cache at the terminal", async () => {
    const harness = fakeHarness({ cacheReady: false });
    const session = startSession(harness);
    harness.close(4401);
    expect(session.getCacheEmpty()).toBe(true);
    await flush();
    expect(harness.fetchBinary).toHaveBeenCalledTimes(1);
    expect(session.getFallback()).toBe("done");
  });

  it("fails the fallback on a refused fetch", async () => {
    const harness = fakeHarness();
    harness.fetchBinary.mockRejectedValue(new Error("gone"));
    const session = startSession(harness);
    harness.close(4401);
    await flush();
    expect(session.getFallback()).toBe("failed");
  });

  it("fails the fallback on empty or corrupt bytes", async () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.fetchBinary.mockResolvedValue(new Uint8Array([]));
    harness.close(4401);
    await flush();
    expect(session.getFallback()).toBe("failed");

    const corrupt = fakeHarness();
    corrupt.applyBinary.mockImplementation(() => {
      throw new Error("corrupt");
    });
    const retry = createPageLiveSession({
      pageId: PAGE_ID,
      backend: corrupt.backend,
      fetchBinary: corrupt.fetchBinary,
    });
    retry.start();
    corrupt.close(4401);
    await flush();
    expect(retry.getFallback()).toBe("failed");
    retry.destroy();
  });

  it("freezes the fallback when destroyed mid-fetch", async () => {
    const harness = fakeHarness();
    let release!: (bytes: Uint8Array) => void;
    harness.fetchBinary.mockReturnValue(
      new Promise<Uint8Array>((resolve) => {
        release = resolve;
      })
    );
    const session = startSession(harness);
    harness.close(4401);
    await flush();
    expect(session.getFallback()).toBe("loading");
    session.destroy();
    release(new Uint8Array([9]));
    await flush();
    expect(session.getFallback()).toBe("loading");
  });

  it("fast-forwards a pending retry on nudge", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    session.nudge();
    expect(harness.connect).toHaveBeenCalledTimes(1);
    harness.open();
    harness.synced();
    session.nudge();
    expect(harness.connect).toHaveBeenCalledTimes(1);
    harness.close(1006);
    session.nudge();
    expect(harness.connect).toHaveBeenCalledTimes(2);
    session.nudge();
    expect(harness.connect).toHaveBeenCalledTimes(2);
  });

  it("ignores a nudge off a session", () => {
    const harness = fakeHarness();
    const session = createPageLiveSession({
      pageId: PAGE_ID,
      backend: harness.backend,
      fetchBinary: harness.fetchBinary,
    });
    session.nudge();
    expect(harness.connect).not.toHaveBeenCalled();
    session.start();
    session.destroy();
    session.nudge();
    expect(harness.connect).toHaveBeenCalledTimes(1);
  });

  it("redials a healthy session on reconnect with a fresh budget", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    harness.synced();
    session.reconnect();
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
    expect(sync.reportPageSync).toHaveBeenLastCalledWith(PAGE_ID, "syncing");
    expect(harness.connect).toHaveBeenCalledTimes(2);
    expect(harness.disconnect).toHaveBeenCalledTimes(2);
    // The forced redial resets the retry budget: the next drop counts
    // from one again.
    harness.close(1006);
    expect(session.getStage()).toEqual({ kind: "reconnecting", attempt: 1 });
  });

  it("redials a terminal session on reconnect", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    harness.open();
    harness.synced();
    harness.authFailed();
    expect(session.getStage().kind).toBe("disconnected");
    session.reconnect();
    expect(session.getStage()).toEqual({ kind: "connecting" });
    expect(harness.connect).toHaveBeenCalledTimes(2);
    harness.open();
    harness.synced();
    expect(session.getStage()).toEqual({ kind: "synced" });
  });

  it("ignores a reconnect off a session", () => {
    const harness = fakeHarness();
    const session = createPageLiveSession({
      pageId: PAGE_ID,
      backend: harness.backend,
      fetchBinary: harness.fetchBinary,
    });
    session.reconnect();
    expect(harness.connect).not.toHaveBeenCalled();
    session.start();
    session.destroy();
    session.reconnect();
    expect(harness.connect).toHaveBeenCalledTimes(1);
  });

  it("tears everything down on destroy", () => {
    const harness = fakeHarness();
    const session = startSession(harness);
    const announced: string[] = [];
    const stop = session.subscribe(() => {
      announced.push(session.getStage().kind);
    });
    harness.close(1006);
    expect(announced).toEqual(["reconnecting"]);
    stop();
    session.destroy();
    session.destroy();
    expect(harness.destroyBackend).not.toHaveBeenCalled();
    harness.open();
    harness.close(1006);
    harness.synced();
    harness.authFailed();
    vi.advanceTimersByTime(120_000);
    expect(announced).toEqual(["reconnecting"]);
    expect(harness.connect).toHaveBeenCalledTimes(1);
  });

  it("announces subscribes and initial reads", () => {
    const harness = fakeHarness();
    const session = createPageLiveSession({
      pageId: PAGE_ID,
      backend: harness.backend,
      fetchBinary: harness.fetchBinary,
    });
    expect(session.getStage()).toEqual({ kind: "initial" });
    expect(session.getCacheEmpty()).toBeNull();
    expect(session.getFallback()).toBe("idle");
  });
});

describe("page property events", () => {
  it("forwards local lock events over the socket once open", () => {
    const harness = fakeHarness();
    startSession(harness);
    harness.open();

    publishProperty(PAGE_ID, "locked");
    publishProperty(PAGE_ID, "unlocked");

    expect(harness.sendStateless).toHaveBeenCalledTimes(2);
    expect(harness.sendStateless).toHaveBeenNthCalledWith(1, "lock");
    expect(harness.sendStateless).toHaveBeenNthCalledWith(2, "unlock");
  });

  it("holds lock events while the socket is down and flushes the latest on open", () => {
    const harness = fakeHarness();
    startSession(harness);

    publishProperty(PAGE_ID, "locked");
    publishProperty(PAGE_ID, "unlocked");
    expect(harness.sendStateless).not.toHaveBeenCalled();

    harness.open();
    expect(harness.sendStateless).toHaveBeenCalledTimes(1);
    expect(harness.sendStateless).toHaveBeenCalledWith("unlock");
  });

  it("ignores foreign pages, malformed payloads and non-property events", () => {
    const harness = fakeHarness();
    startSession(harness);
    harness.open();
    const hub = getRealtimeHub();

    publishProperty("other-page", "locked");
    hub.publish({ topic: PAGES_REALTIME_TOPIC, payload: null });
    hub.publish({ topic: PAGES_REALTIME_TOPIC, payload: "locked" });
    hub.publish({ topic: PAGES_REALTIME_TOPIC, payload: { pageId: PAGE_ID, event: 7 } });
    publishProperty(PAGE_ID, "archived");

    expect(harness.sendStateless).not.toHaveBeenCalled();
  });

  it("delivers peers' property events and swallows its own echo", () => {
    const harness = fakeHarness();
    const onPropertyEvent = vi.fn();
    startSession(harness, onPropertyEvent);
    harness.open();

    harness.stateless("locked");
    expect(onPropertyEvent).toHaveBeenCalledTimes(1);
    expect(onPropertyEvent).toHaveBeenCalledWith("locked");

    publishProperty(PAGE_ID, "locked");
    expect(harness.sendStateless).toHaveBeenCalledWith("lock");
    harness.stateless("locked");
    expect(onPropertyEvent).toHaveBeenCalledTimes(1);

    harness.stateless("locked");
    expect(onPropertyEvent).toHaveBeenCalledTimes(2);
  });

  it("ignores unknown bare broadcasts without touching the error path", () => {
    const harness = fakeHarness();
    const onPropertyEvent = vi.fn();
    const errors: unknown[] = [];
    const stop = subscribePageRealtimeErrors((error) => {
      errors.push(error);
    });
    try {
      startSession(harness, onPropertyEvent);

      harness.stateless("archived");
      harness.stateless("");

      expect(onPropertyEvent).not.toHaveBeenCalled();
      expect(errors).toEqual([]);
      expect(shell.notifyError).not.toHaveBeenCalled();
    } finally {
      stop();
    }
  });

  it("tolerates property events with no listener attached", () => {
    const harness = fakeHarness();
    startSession(harness);

    expect(() => harness.stateless("locked")).not.toThrow();
  });

  it("drops pending echoes on close and stops forwarding on destroy", () => {
    const harness = fakeHarness();
    const onPropertyEvent = vi.fn();
    const session = startSession(harness, onPropertyEvent);
    harness.open();
    publishProperty(PAGE_ID, "locked");
    expect(harness.sendStateless).toHaveBeenCalledTimes(1);

    harness.close(1006);
    harness.stateless("locked");
    expect(onPropertyEvent).toHaveBeenCalledTimes(1);

    session.destroy();
    publishProperty(PAGE_ID, "unlocked");
    expect(harness.sendStateless).toHaveBeenCalledTimes(1);
  });
});
