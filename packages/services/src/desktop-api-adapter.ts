/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import axios, { AxiosError, AxiosHeaders, CanceledError, getAdapter, isAxiosError } from "axios";
import type { AxiosAdapter, AxiosResponse } from "axios";

export type NativeResponseHead = {
  status: number;
  statusText: string;
  headers: [string, string][];
};
type NativeChannel<T> = { onmessage: (message: T) => void };
type NativeCore = {
  invoke<T>(command: string, args?: Record<string, unknown> | Uint8Array): Promise<T>;
  Channel: new <T>() => NativeChannel<T>;
};
type NativeWindow = Window & {
  __PIDASH_NATIVE_HTTP__?: string;
  __PIDASH_NATIVE_SESSION__?: { refresh?: boolean; strict?: boolean };
  __TAURI__?: { core: NativeCore };
};
export type NativeApi = {
  apiOrigin: string;
  core: NativeCore;
  /** The transport answers a 401 with a session refresh and a replay itself,
   * so a 401 that reaches the page is final and the page must not refresh. */
  refreshesSession: boolean;
  /** The page is the bundle, cross-site to the API: the webview's own
   * networking cannot carry the session, so it is not a fallback. */
  strict: boolean;
};

/** A credentialed request could not take the native route. In the bundled
 * app the webview's own networking would send it without the session cookies
 * and the server would answer 401, so it fails here instead, naming both
 * origins: they differ when the web bundle and the binary were built for
 * different servers (`VITE_API_BASE_URL` / `PI_DASH_URL`). */
export class DesktopTransportError extends Error {
  readonly requestOrigin: string;

  constructor(
    readonly url: string,
    readonly apiOrigin: string
  ) {
    const target = new URL(url, window.location.href);
    super(
      `Credentialed request to ${target.origin}${target.pathname} cannot use the desktop app's native transport, ` +
        `which only serves ${apiOrigin}/api/ and ${apiOrigin}/auth/. ` +
        `Request origin: ${target.origin}; configured API origin: ${apiOrigin}.`
    );
    this.name = "DesktopTransportError";
    this.requestOrigin = target.origin;
  }
}

// Mirrors the Rust side's error strings (desktop_http.rs).
const CANCELED = "API request canceled";
const TIMED_OUT = "API request timed out";

/** The native cookie transport, when this binary advertises it. Browser
 * builds, SSR, unit tests and older desktop binaries get undefined.
 */
export function getNativeApi(): NativeApi | undefined {
  if (typeof window === "undefined") return undefined;
  const native = window as NativeWindow;
  if (typeof native.__PIDASH_NATIVE_HTTP__ !== "string" || !native.__TAURI__?.core) return undefined;
  const session = native.__PIDASH_NATIVE_SESSION__;
  return {
    apiOrigin: native.__PIDASH_NATIVE_HTTP__,
    core: native.__TAURI__.core,
    refreshesSession: session?.refresh === true,
    strict: session?.strict === true,
  };
}

/** `url` (resolved like the webview would) when it targets the native API
 * allowlist; undefined when the native transport cannot carry it. */
export function nativeApiUrl(native: NativeApi, url: string): URL | undefined {
  const target = new URL(url, window.location.href);
  if (target.origin !== native.apiOrigin) return undefined;
  if (!(target.pathname.startsWith("/api/") || target.pathname.startsWith("/auth/"))) return undefined;
  return target;
}

let sequence = 0;
export function nextNativeRequestId(): string {
  sequence += 1;
  return `${Date.now().toString(36)}-${sequence.toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export function cancelNativeRequest(native: NativeApi, id: string): void {
  native.core.invoke("desktop_api_cancel", { id }).catch(() => undefined);
}

// Raw IPC frames: 4-byte big-endian head length, JSON head, then the body.
function frame(head: unknown, body: Uint8Array | null): Uint8Array {
  const encoded = new TextEncoder().encode(JSON.stringify(head));
  const out = new Uint8Array(4 + encoded.length + (body?.length ?? 0));
  new DataView(out.buffer).setUint32(0, encoded.length);
  out.set(encoded, 4);
  if (body) out.set(body, 4 + encoded.length);
  return out;
}

function unframe(result: ArrayBuffer | number[]): { head: NativeResponseHead; body: Uint8Array } {
  const bytes = result instanceof ArrayBuffer ? new Uint8Array(result) : new Uint8Array(result);
  const length = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0);
  const head = JSON.parse(new TextDecoder().decode(bytes.subarray(4, 4 + length))) as NativeResponseHead;
  return { head, body: bytes.subarray(4 + length) };
}

/** Use the native cookie transport only when this binary advertises it.
 *
 * The webview's own adapter is kept for requests that must not carry the
 * session: presigned storage uploads and anything else sent with
 * `withCredentials: false` (with its progress and cancellation), and
 * requests an instance without credentials makes to other hosts. A
 * credentialed request the native transport cannot carry throws
 * {@link DesktopTransportError} in the bundled app.
 */
export function getDesktopApiAdapter(): AxiosAdapter | undefined {
  const native = getNativeApi();
  if (!native) return undefined;
  const browserAdapter = getAdapter(axios.defaults.adapter);

  return async (config) => {
    if (config.withCredentials === false) return browserAdapter(config);
    const url = axios.getUri(config);
    const target = nativeApiUrl(native, url);
    if (!target) {
      if (config.withCredentials && native.strict) throw new DesktopTransportError(url, native.apiOrigin);
      return browserAdapter(config);
    }
    if (config.signal?.aborted) throw new CanceledError();
    config.cancelToken?.throwIfRequested();
    const headers = new AxiosHeaders(config.headers);
    let body: Uint8Array | null = null;
    if (config.data != null) {
      // Axios already transforms JSON to a string. Request/Response handles
      // FormData, Blob, ArrayBuffer and URLSearchParams without losing bytes.
      const form = typeof FormData !== "undefined" && config.data instanceof FormData;
      const encoded = new Response(config.data);
      if (form) headers.setContentType(encoded.headers.get("content-type"));
      body = new Uint8Array(await encoded.arrayBuffer());
    }
    if (config.signal?.aborted) throw new CanceledError();
    config.cancelToken?.throwIfRequested();
    const id = nextNativeRequestId();
    const request = frame(
      {
        id,
        url: target.href,
        method: (config.method ?? "get").toUpperCase(),
        headers: Object.entries(headers.toJSON())
          .filter(([, value]) => value != null && value !== false)
          .map(([name, value]) => [name, String(value)]),
        hasBody: body !== null,
        timeoutMs: config.timeout || null,
      },
      body
    );
    const timeoutError = () =>
      new AxiosError(`timeout of ${config.timeout}ms exceeded`, AxiosError.ECONNABORTED, config);
    let cancel: (() => void) | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const canceled = new Promise<never>((_resolve, reject) => {
      const stop = (error: Error) => {
        cancelNativeRequest(native, id);
        reject(error);
      };
      cancel = () => stop(new CanceledError());
      config.signal?.addEventListener?.("abort", cancel);
      config.cancelToken?.promise.then(() => cancel?.());
      if (config.timeout) timeout = setTimeout(() => stop(timeoutError()), config.timeout);
    });
    try {
      const result = await Promise.race([
        native.core.invoke<ArrayBuffer | number[]>("desktop_api_request", request),
        canceled,
      ]);
      const { head, body: bytes } = unframe(result);
      const responseHeaders = new AxiosHeaders(Object.fromEntries(head.headers));
      const data =
        config.responseType === "arraybuffer"
          ? bytes.slice().buffer
          : config.responseType === "blob"
            ? new Blob([bytes], { type: String(responseHeaders.getContentType() ?? "") })
            : new TextDecoder().decode(bytes);
      const response: AxiosResponse = {
        data,
        status: head.status,
        statusText: head.statusText,
        headers: responseHeaders,
        config,
      };
      if (config.validateStatus && !config.validateStatus(response.status)) {
        throw new AxiosError(
          `Request failed with status code ${response.status}`,
          response.status >= 500 ? AxiosError.ERR_BAD_RESPONSE : AxiosError.ERR_BAD_REQUEST,
          config,
          undefined,
          response
        );
      }
      return response;
    } catch (error) {
      if (isAxiosError(error)) throw error;
      if (error === CANCELED) throw new CanceledError();
      if (error === TIMED_OUT) throw timeoutError();
      throw new AxiosError(String(error), AxiosError.ERR_NETWORK, config);
    } finally {
      if (timeout) clearTimeout(timeout);
      if (cancel) {
        config.signal?.removeEventListener?.("abort", cancel);
        cancel = undefined;
      }
    }
  };
}
