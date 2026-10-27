// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration session (parity/specs/pages.md §8.4, §8.6, PAGE-067,
// PAGE-069). One backend, one retry schedule, one walk through the
// reconnect machine: the socket dials on its own backoff, frames route
// to the realtime seams, and a terminal disconnect with an empty cache
// fetches the binary fallback. Every redial kills the stale socket
// first; a close that arrives late for a killed dial counts as one more
// failed attempt, which only shortens the invisible retry budget.

import { getPageDescriptionBinary } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/toaster.js";
import {
  createPageLiveBackend,
  PAGE_LIVE_BODY_FIELD,
  PAGE_LIVE_TITLE_FIELD,
  type PageLiveBackend,
  type PageLiveBackendInput,
} from "./pageLiveBackend.js";
import { PAGE_CONTENT_TOO_LARGE, reportPageRealtimeError } from "./pageRealtimeErrors.js";
import { pageLiveCloseKind, parsePageLivePayload, type PageLiveCloseKind } from "./pageLiveFrames.js";
import { liveCursorColor, liveIdentityToken, type PageLiveUser } from "./pageLiveIdentity.js";
import {
  nextPageLiveStage,
  PAGE_LIVE_CONNECT_TIMEOUT_MS,
  PAGE_LIVE_MAX_ATTEMPTS,
  pageLiveRetryDelayMs,
  pageLiveSyncReport,
  type PageLiveStage,
} from "./pageLiveStages.js";
import { reportPageSync } from "./pageSync.js";
import { pageLiveSocketUrl } from "./pageLiveUrl.js";

/** A persist failure the channel carried. */
export const PAGE_LIVE_SAVE_FAILED_TITLE = "Could not save to the live document.";

/** A load failure the channel carried. */
export const PAGE_LIVE_LOAD_FAILED_TITLE = "Could not load the live document.";

export type PageLiveFallbackState = "idle" | "loading" | "done" | "failed";

export interface PageLiveSession {
  getStage(): PageLiveStage;
  getCacheEmpty(): boolean | null;
  getFallback(): PageLiveFallbackState;
  subscribe(listener: () => void): () => void;
  /** Fast-forward a pending retry (focus regained). */
  nudge(): void;
  /** Force a fresh dial with a new handshake (connectivity regained). */
  reconnect(): void;
  start(): void;
  destroy(): void;
}

export interface PageLiveSessionDeps {
  pageId: string;
  backend: PageLiveBackend;
  fetchBinary: () => Promise<Uint8Array>;
}

/**
 * One page's session over an open backend. Reports every stage walk to
 * the sync seam, every decoded error to the realtime seam, and every
 * distinct failure as a notice — except the oversized code, whose strip
 * owns its surface (PAGE-063).
 */
export function createPageLiveSession(deps: PageLiveSessionDeps): PageLiveSession {
  const { pageId, backend } = deps;
  let stage: PageLiveStage = { kind: "initial" };
  let retriesUsed = 0;
  let cacheEmpty: boolean | null = null;
  let fallback: PageLiveFallbackState = "idle";
  let started = false;
  let destroyed = false;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let connectTimer: ReturnType<typeof setTimeout> | null = null;
  let lastNotice = "";
  let stoppers: Array<() => void> = [];
  const listeners = new Set<() => void>();

  const emit = (): void => {
    for (const listener of listeners) listener();
  };

  const transition = (event: Parameters<typeof nextPageLiveStage>[1]): void => {
    const next = nextPageLiveStage(stage, event);
    if (next === stage) return;
    stage = next;
    reportPageSync(pageId, pageLiveSyncReport(stage));
    emit();
  };

  const clearRetry = (): void => {
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  };

  const clearConnect = (): void => {
    if (connectTimer !== null) {
      clearTimeout(connectTimer);
      connectTimer = null;
    }
  };

  const evaluateCache = (): void => {
    if (cacheEmpty !== null) return;
    cacheEmpty =
      backend.doc.fragmentLength(PAGE_LIVE_BODY_FIELD) === 0 && backend.doc.fragmentLength(PAGE_LIVE_TITLE_FIELD) === 0;
    emit();
  };

  const failFallback = (): void => {
    fallback = "failed";
    emit();
  };

  /** Terminal with an empty cache: render the separately fetched binary. */
  const settleTerminal = async (): Promise<void> => {
    if (cacheEmpty === null) evaluateCache();
    if (cacheEmpty === false) return;
    fallback = "loading";
    emit();
    let bytes: Uint8Array | null = null;
    try {
      bytes = await deps.fetchBinary();
    } catch {
      bytes = null;
    }
    if (destroyed) return;
    if (bytes === null || bytes.length === 0) {
      failFallback();
      return;
    }
    try {
      backend.doc.applyBinary(bytes);
    } catch {
      failFallback();
      return;
    }
    fallback = "done";
    emit();
  };

  const enterFailed = (kind: PageLiveCloseKind): void => {
    // Every caller holds a live stage (post-terminal drops return in
    // the handler), so this always lands on a fresh disconnected.
    transition({
      type: "failed",
      reason: kind === "auth" ? "auth" : kind === "force" ? "force-close" : "exhausted",
    });
    void settleTerminal();
  };

  const dial = (): void => {
    clearRetry();
    backend.connection.disconnect();
    // A dial that never sees a message rejects instead of closing: the
    // socket reports those failures through the dial, not the channel.
    void backend.connection.connect().then(undefined, () => handleDrop("retryable"));
    clearConnect();
    connectTimer = setTimeout(() => {
      connectTimer = null;
      handleDrop("retryable");
    }, PAGE_LIVE_CONNECT_TIMEOUT_MS);
  };

  const handleDrop = (kind: PageLiveCloseKind): void => {
    if (destroyed || stage.kind === "disconnected") return;
    clearConnect();
    if (kind === "auth" || kind === "force") {
      // A rejected handshake and a forced close never redial: the server
      // already decided this socket is over (PAGE-068, PAGE-069).
      clearRetry();
      enterFailed(kind);
      return;
    }
    if (retriesUsed >= PAGE_LIVE_MAX_ATTEMPTS) {
      clearRetry();
      enterFailed(kind);
      return;
    }
    retriesUsed += 1;
    transition({ type: "dropped", attempt: retriesUsed });
    clearRetry();
    retryTimer = setTimeout(() => {
      retryTimer = null;
      dial();
    }, pageLiveRetryDelayMs(retriesUsed));
  };

  const handleOpen = (): void => {
    // A socket that opens proves the network is back: past drops stop
    // counting against the retry budget (PAGE-067).
    retriesUsed = 0;
    clearConnect();
    transition({ type: "opened" });
  };

  const handleClose = (code: number): void => {
    clearConnect();
    handleDrop(pageLiveCloseKind(code));
  };

  const handleAuthFailed = (): void => {
    // A rejected handshake answers with a message and leaves the socket
    // idling, so the terminal lands here: drop the socket before its late
    // close arrives, which the terminal guard then ignores.
    if (stage.kind === "disconnected") return;
    handleDrop("auth");
    backend.connection.disconnect();
  };

  const handleSynced = (): void => {
    // A clean sync re-arms failure notices: a recurring failure after a
    // recovery toasts again instead of staying suppressed.
    lastNotice = "";
    // Converged documents restart the retry budget with them, so a long
    // healthy session never exhausts on old drops (PAGE-067).
    retriesUsed = 0;
    transition({ type: "synced" });
  };

  const handleStateless = (payload: string): void => {
    const inbound = parsePageLivePayload(payload, pageId);
    if (inbound === null || inbound.kind === "force-close") return;
    const error = inbound.error;
    reportPageRealtimeError({
      pageId,
      phase: error.phase,
      code: error.code,
      message: error.message,
      shouldDisconnect: error.shouldDisconnect,
    });
    if (error.code === PAGE_CONTENT_TOO_LARGE) return;
    const notice = `${error.phase}:${error.code}:${error.message}`;
    if (notice === lastNotice) return;
    lastNotice = notice;
    notifyError(error.phase === "load" ? PAGE_LIVE_LOAD_FAILED_TITLE : PAGE_LIVE_SAVE_FAILED_TITLE, error.message);
  };

  return {
    getStage: () => stage,
    getCacheEmpty: () => cacheEmpty,
    getFallback: () => fallback,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    nudge: () => {
      if (destroyed || !started) return;
      if (stage.kind !== "reconnecting" || retryTimer === null) return;
      clearRetry();
      dial();
    },
    reconnect: () => {
      if (destroyed || !started) return;
      // A regained connection always earns a fresh handshake: the
      // credentials may have changed while the socket idled, so even a
      // healthy-looking socket is redialled rather than trusted (the
      // handshake re-validates the session cookie, PAGE-068).
      retriesUsed = 0;
      if (stage.kind === "disconnected") transition({ type: "reset" });
      if (stage.kind === "initial") transition({ type: "start" });
      else transition({ type: "dropped", attempt: 1 });
      dial();
    },
    start: () => {
      if (started) return;
      started = true;
      transition({ type: "start" });
      if (backend.cache.isReady()) evaluateCache();
      else stoppers.push(backend.cache.onReady(evaluateCache));
      stoppers.push(
        backend.connection.onOpen(handleOpen),
        backend.connection.onClose(handleClose),
        backend.connection.onSynced(handleSynced),
        backend.connection.onStateless(handleStateless),
        backend.connection.onAuthFailed(handleAuthFailed)
      );
      dial();
    },
    destroy: () => {
      if (destroyed) return;
      destroyed = true;
      clearRetry();
      clearConnect();
      for (const stop of stoppers) stop();
      stoppers = [];
    },
  };
}

export interface UsePageLiveSessionOptions {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  /** Null while signed out: no credentials open no socket (PAGE-068). */
  user: PageLiveUser | null;
  /**
   * False where no session runs: panels, and reads the identity never
   * arrived for. Read-only viewers join the session and render it without
   * input (PAGE-053, PAGE-092).
   */
  enabled: boolean;
  /** Tests inject a stub backend; production loads the chunk. */
  loadBackend?: ((input: PageLiveBackendInput) => Promise<PageLiveBackend>) | undefined;
  /** Tests inject stored bytes; production reads the description. */
  fetchBinary?: (() => Promise<Uint8Array>) | undefined;
}

/** What the editable surface mounts the live editors from. */
export interface PageLiveMount {
  stage: PageLiveStage;
  /** The document handle for the editor bindings; null until loaded. */
  docHandle: unknown;
  /** Null until the cache resolves; the loader shows while not false. */
  cacheEmpty: boolean | null;
  fallback: PageLiveFallbackState;
  /** The editors mount once the document is known. */
  liveReady: boolean;
  showLoader: boolean;
  /** Seed empty fields from stored content: the server state is known. */
  seedDocument: boolean;
}

function loadingMount(): PageLiveMount {
  return {
    stage: { kind: "initial" },
    docHandle: null,
    cacheEmpty: null,
    fallback: "idle",
    liveReady: false,
    showLoader: true,
    seedDocument: false,
  };
}

/** Read the stored binary the disconnected fallback renders (PAGE-067). */
export function fetchPageLiveFallbackBinary(
  workspaceSlug: string,
  projectId: string,
  pageId: string
): Promise<Uint8Array> {
  return getPageDescriptionBinary(getAppClient(), workspaceSlug, projectId, pageId);
}

function degradedMount(): PageLiveMount {
  return {
    stage: { kind: "initial" },
    docHandle: null,
    cacheEmpty: true,
    fallback: "failed",
    liveReady: true,
    showLoader: false,
    seedDocument: false,
  };
}

function snapshot(session: PageLiveSession, backend: PageLiveBackend): PageLiveMount {
  const stage = session.getStage();
  const cacheEmpty = session.getCacheEmpty();
  const fallback = session.getFallback();
  const liveReady = stage.kind === "synced" || cacheEmpty === false || fallback === "done" || fallback === "failed";
  return {
    stage,
    docHandle: backend.doc.handle,
    cacheEmpty,
    fallback,
    liveReady,
    showLoader: !liveReady,
    seedDocument: stage.kind === "synced" || fallback === "done" || fallback === "failed",
  };
}

/**
 * One page's collaboration session for the detail surface, editable and
 * read-only alike: loads the backend chunk, walks the reconnect machine,
 * and mirrors the mount the live editors bind to — read-only viewers bind
 * it with editing off (PAGE-092). Null while disabled or signed out. A
 * chunk that fails to load degrades to the standalone editor with the
 * fallback save, never to a stuck loader.
 */
export function usePageLiveSession(options: UsePageLiveSessionOptions): PageLiveMount | null {
  const { workspaceSlug, projectId, pageId, user, enabled } = options;
  const userId = user?.id ?? null;
  const userName = user?.name ?? "";
  const loadRef = React.useRef(options.loadBackend);
  loadRef.current = options.loadBackend;
  const fetchRef = React.useRef(options.fetchBinary);
  fetchRef.current = options.fetchBinary;
  const sessionRef = React.useRef<PageLiveSession | null>(null);

  const liveKey = `${pageId}:${enabled}:${userId ?? ""}`;
  const [trackedKey, setTrackedKey] = React.useState(liveKey);
  const [mount, setMount] = React.useState<PageLiveMount | null>(() =>
    enabled && user !== null ? loadingMount() : null
  );
  if (trackedKey !== liveKey) {
    setTrackedKey(liveKey);
    setMount(enabled && user !== null ? loadingMount() : null);
  }

  React.useEffect(() => {
    if (!enabled || userId === null) return;
    reportPageSync(pageId, "syncing");
    let cancelled = false;
    let session: PageLiveSession | null = null;
    let backend: PageLiveBackend | null = null;
    let stopSync: (() => void) | null = null;
    const load = loadRef.current ?? createPageLiveBackend;
    const readBinary = fetchRef.current ?? (() => fetchPageLiveFallbackBinary(workspaceSlug, projectId, pageId));
    void load({
      url: pageLiveSocketUrl(getAppClient().baseUrl, { workspaceSlug, projectId }),
      documentName: pageId,
      token: liveIdentityToken({ id: userId, name: userName }),
      userName,
      userColor: liveCursorColor(userId),
      cacheName: `page-live-${pageId}`,
    }).then(
      (ready) => {
        if (cancelled) {
          ready.destroy();
          return;
        }
        backend = ready;
        const liveSession = createPageLiveSession({ pageId, backend: ready, fetchBinary: readBinary });
        session = liveSession;
        sessionRef.current = liveSession;
        const sync = (): void => setMount(snapshot(liveSession, ready));
        stopSync = liveSession.subscribe(sync);
        liveSession.start();
        sync();
      },
      () => {
        if (cancelled) return;
        // The chunk failed: the fault badge owns the surface and the
        // fallback owns persistence, never a stuck syncing beat.
        reportPageSync(pageId, "lost");
        setMount(degradedMount());
      }
    );
    return () => {
      cancelled = true;
      sessionRef.current = null;
      stopSync?.();
      session?.destroy();
      backend?.destroy();
    };
  }, [workspaceSlug, projectId, pageId, enabled, userId, userName]);

  React.useEffect(() => {
    if (!enabled || userId === null) return;
    const nudge = (): void => sessionRef.current?.nudge();
    const reconnect = (): void => sessionRef.current?.reconnect();
    const visible = (): void => {
      if (document.visibilityState === "visible") nudge();
    };
    window.addEventListener("online", reconnect);
    window.addEventListener("focus", nudge);
    document.addEventListener("visibilitychange", visible);
    return () => {
      window.removeEventListener("online", reconnect);
      window.removeEventListener("focus", nudge);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [enabled, userId]);

  return mount;
}

/**
 * The sessionless half of the session contract: a page no session runs
 * for reports converged, so the fault badge never owns a surface the
 * socket was never meant to drive (PAGE-067). The screen keeps it off
 * once a session owns the page's reports, on the read-only side too.
 */
export function usePageReadOnlySyncReport(pageId: string, snapshot: boolean): void {
  React.useEffect(() => {
    if (!snapshot) return;
    reportPageSync(pageId, "synced");
  }, [snapshot, pageId]);
}
