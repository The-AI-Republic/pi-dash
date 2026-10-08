// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop platform: API traffic goes through the Rust HTTP layer (cookies
// stay out of the page), streams relay through its streaming command, and
// storage persists through the webview profile (file-backed under the app
// data directory). This module is excluded from the web bundle by the
// build-time alias; the no-Tauri bundle check proves it.

import { createPersistentStore } from "./storage.js";
import type { EventStream, Platform, StreamInit, StreamMessage, TitleBarApi, Unsubscribe } from "./types.js";

/** Subset of the Tauri webview globals this module touches. */
interface TauriInvokeCore {
  invoke<T>(command: string, payload?: unknown): Promise<T>;
  Channel: new <T>() => { onmessage: ((message: T) => void) | null };
}

interface TauriWindow {
  __PIDASH_NATIVE_HTTP__?: unknown;
  __TAURI__?: { core?: TauriInvokeCore };
}

function tauriWindow(): (Window & TauriWindow) | undefined {
  if (typeof globalThis.window === "undefined") return undefined;
  return globalThis.window as Window & TauriWindow;
}

/** The desktop shell advertises its API origin; without it we are not desktop. */
function nativeApiOrigin(): { origin: string; core: TauriInvokeCore } | undefined {
  const win = tauriWindow();
  const origin = win?.__PIDASH_NATIVE_HTTP__;
  const core = win?.__TAURI__?.core;
  if (typeof origin !== "string" || !core) return undefined;
  return { origin, core };
}

function isApiPath(pathname: string): boolean {
  return pathname.startsWith("/api/") || pathname.startsWith("/auth/");
}

let requestSequence = 0;

function nextRequestId(): string {
  requestSequence += 1;
  return `${Date.now().toString(36)}.${requestSequence.toString(36)}`;
}

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

/** Wire framing: 4-byte big-endian head length, JSON head, raw body. */
export function encodeFrame(head: unknown, body: Uint8Array | null): Uint8Array {
  const headBytes = textEncoder.encode(JSON.stringify(head));
  const out = new Uint8Array(4 + headBytes.length + (body?.length ?? 0));
  new DataView(out.buffer).setUint32(0, headBytes.length);
  out.set(headBytes, 4);
  if (body) out.set(body, 4 + headBytes.length);
  return out;
}

export function decodeFrame(bytes: Uint8Array): { head: unknown; body: Uint8Array } {
  if (bytes.length < 4) throw new Error("Truncated response frame");
  const headLength = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
  if (bytes.length < 4 + headLength) throw new Error("Truncated response frame");
  const head = JSON.parse(textDecoder.decode(bytes.subarray(4, 4 + headLength))) as unknown;
  return { head, body: bytes.subarray(4 + headLength) };
}

interface NativeResponseHead {
  status?: number;
  statusText?: string;
  status_text?: string;
  headers?: Array<[string, string]>;
}

function headToResponse(head: NativeResponseHead, body: Uint8Array): Response {
  const headers = new Headers();
  for (const [name, value] of head.headers ?? []) {
    try {
      headers.append(name, value);
    } catch {
      // Skip response headers the page cannot represent.
    }
  }
  const status = head.status ?? 200;
  const statusText = head.statusText ?? head.status_text ?? "";
  return new Response(status === 204 || status === 304 ? null : (body as unknown as BodyInit), {
    status,
    statusText,
    headers,
  });
}

async function nativeRequest(
  core: TauriInvokeCore,
  url: string,
  init: RequestInit | undefined,
  signal: AbortSignal | null
): Promise<Response> {
  const id = nextRequestId();
  const method = (init?.method ?? "GET").toUpperCase();
  const headers = new Headers(init?.headers);
  const pairs: Array<[string, string]> = [];
  headers.forEach((value, name) => pairs.push([name, value]));
  const bodyBytes =
    init?.body == null
      ? null
      : typeof init.body === "string"
        ? textEncoder.encode(init.body)
        : new Uint8Array(await new Response(init.body as BodyInit).arrayBuffer());
  const frame = encodeFrame({ id, url, method, headers: pairs, has_body: bodyBytes !== null }, bodyBytes);
  const cancel = (): void => {
    core.invoke("desktop_api_cancel", { id }).catch(() => undefined);
  };
  signal?.addEventListener("abort", cancel, { once: true });
  try {
    if (signal?.aborted) throw signal.reason ?? new DOMException("Aborted", "AbortError");
    const raw = await core.invoke<ArrayBuffer | number[]>("desktop_api_request", frame);
    const bytes = raw instanceof ArrayBuffer ? new Uint8Array(raw) : new Uint8Array(raw);
    const { head, body } = decodeFrame(bytes);
    return headToResponse(head as NativeResponseHead, body);
  } finally {
    signal?.removeEventListener("abort", cancel);
  }
}

function desktopFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const native = nativeApiOrigin();
  const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
  const request = input instanceof Request ? input : undefined;
  const signal = init?.signal ?? request?.signal ?? null;
  if (!native) return globalThis.fetch(input, init);
  let target: URL | undefined;
  try {
    target = new URL(url, globalThis.location.href);
  } catch {
    return globalThis.fetch(input, init);
  }
  if (target.origin !== native.origin || !isApiPath(target.pathname)) {
    return globalThis.fetch(input, init);
  }
  const merged: RequestInit = { method: init?.method ?? request?.method ?? "GET" };
  const headers = init?.headers ?? request?.headers;
  if (headers !== undefined) {
    merged.headers = headers;
  }
  const body = init?.body ?? request?.body;
  if (body !== undefined && body !== null) {
    merged.body = body;
  }
  return nativeRequest(native.core, target.toString(), merged, signal);
}

type NativeStreamEvent = {
  type: string;
  status?: number;
  statusText?: string;
  headers?: Array<[string, string]>;
  text?: string;
};

function desktopStream(url: string, init: StreamInit = {}): EventStream {
  const native = nativeApiOrigin();
  if (!native) {
    throw new Error("Desktop streams need the desktop shell");
  }
  const core = native.core;
  const id = nextRequestId();
  const pairs = Object.entries(init.headers ?? {});
  const queue: StreamMessage[] = [];
  let waiting: (() => void) | null = null;
  let finished = false;
  let failed: Error | undefined;

  const wake = (): void => {
    const notify = waiting;
    waiting = null;
    notify?.();
  };

  const channel = new core.Channel<NativeStreamEvent>();
  channel.onmessage = (message) => {
    if (message.type === "head") {
      const status = message.status ?? 200;
      if (status < 200 || status >= 300) {
        failed = new Error(`Stream failed with status ${status}`);
        finished = true;
      }
    } else if (message.type === "data") {
      queue.push({ event: "message", data: message.text ?? "" });
    } else if (message.type === "end") {
      finished = true;
    }
    wake();
  };

  const started = core
    .invoke("desktop_api_stream", {
      request: { id, url, method: init.method ?? "GET", headers: pairs, has_body: init.body != null },
      channel,
    })
    .catch((error: unknown) => {
      failed = error instanceof Error ? error : new Error("Stream failed");
      finished = true;
      wake();
    });

  void started;
  let closed = false;

  const messages = async function* (): AsyncIterable<StreamMessage> {
    try {
      for (;;) {
        if (init.signal?.aborted) return;
        const next = queue.shift();
        if (next) {
          yield next;
          continue;
        }
        if (failed) return;
        if (finished) return;
        await new Promise<void>((resolve) => {
          waiting = resolve;
        });
      }
    } finally {
      close();
    }
  };

  function close(): void {
    if (closed) return;
    closed = true;
    finished = true;
    core.invoke("desktop_api_cancel", { id }).catch(() => undefined);
    wake();
  }

  init.signal?.addEventListener("abort", close, { once: true });
  return { messages, close };
}

/**
 * The shell draws macOS traffic lights over the page, so the title bar
 * must leave room for them. Read from the browser UA surface (no Tauri
 * dependency in this module): userAgentData first, legacy platform after.
 */
function isMacOs(): boolean {
  const nav = globalThis.navigator as (Navigator & { userAgentData?: { platform?: string } }) | undefined;
  if (!nav) return false;
  const hinted = nav.userAgentData?.platform;
  if (typeof hinted === "string" && hinted.length > 0) return hinted.toLowerCase().startsWith("mac");
  return (nav.platform ?? "").toLowerCase().startsWith("mac");
}

const desktopWindow: TitleBarApi = {
  trafficLightInset: isMacOs(),
  setTitle(title: string): void {
    if (typeof globalThis.document !== "undefined") {
      globalThis.document.title = title;
    }
    // Best-effort native title (the shell starts on "Pi Dash"): a missing
    // or older shell rejects the invoke and the document title still wins.
    const core = nativeApiOrigin()?.core;
    if (!core) return;
    core.invoke("plugin:window|set_title", { label: "main", value: title }).catch(() => undefined);
  },
};

function trackFocus(callback: (focused: boolean) => void): Unsubscribe {
  if (typeof globalThis.window === "undefined") return () => undefined;
  const report = (): void => callback(!globalThis.document.hidden);
  globalThis.window.addEventListener("focus", report);
  globalThis.window.addEventListener("blur", report);
  globalThis.document.addEventListener("visibilitychange", report);
  return () => {
    globalThis.window.removeEventListener("focus", report);
    globalThis.window.removeEventListener("blur", report);
    globalThis.document.removeEventListener("visibilitychange", report);
  };
}

export const platform: Platform = {
  kind: "desktop",
  fetch: (input, init) => desktopFetch(input, init),
  stream: (url, init) => desktopStream(url, init),
  openExternal: (url) => {
    const core = nativeApiOrigin()?.core;
    if (!core) {
      globalThis.window.open(url, "_blank", "noopener");
      return Promise.resolve();
    }
    return core.invoke("plugin:opener|open_url", { url }).then(
      () => undefined,
      () => {
        globalThis.window.open(url, "_blank", "noopener");
      }
    );
  },
  storage: createPersistentStore("pidash.desktop"),
  onFocusChange: trackFocus,
  window: desktopWindow,
};
