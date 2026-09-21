/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import axios, { AxiosError, AxiosHeaders, CanceledError, getAdapter, isAxiosError } from "axios";
import type { AxiosAdapter, AxiosResponse } from "axios";

type NativeResponse = {
  status: number;
  statusText: string;
  headers: [string, string][];
  body: number[];
};
type NativeWindow = Window & {
  __PIDASH_NATIVE_HTTP__?: string;
  __TAURI__?: { core: { invoke<T>(command: string, args: Record<string, unknown>): Promise<T> } };
};

/** Use the native cookie transport only when this binary advertises it.
 * Browser builds, SSR, hot-reload tests and older desktop binaries keep their
 * normal Axios adapter. The server's auth/refresh interceptors still run.
 */
export function getDesktopApiAdapter(): AxiosAdapter | undefined {
  if (typeof window === "undefined") return undefined;
  const native = window as NativeWindow;
  if (typeof native.__PIDASH_NATIVE_HTTP__ !== "string" || !native.__TAURI__?.core) return undefined;
  const apiOrigin = native.__PIDASH_NATIVE_HTTP__;
  const browserAdapter = getAdapter(axios.defaults.adapter);
  const { invoke } = native.__TAURI__.core;

  return async (config) => {
    const url = axios.getUri(config);
    const target = new URL(url, apiOrigin);
    // Presigned storage uploads and other external requests must keep the
    // browser adapter (and its progress/cancellation), without API cookies.
    if (
      config.withCredentials === false ||
      target.origin !== apiOrigin ||
      !(target.pathname.startsWith("/api/") || target.pathname.startsWith("/auth/"))
    ) {
      return browserAdapter(config);
    }
    if (config.signal?.aborted) throw new CanceledError();
    config.cancelToken?.throwIfRequested();
    const headers = new AxiosHeaders(config.headers);
    let body: number[] | null = null;
    if (config.data != null) {
      // Axios already transforms JSON to a string. Request/Response handles
      // FormData, Blob, ArrayBuffer and URLSearchParams without losing bytes.
      const form = typeof FormData !== "undefined" && config.data instanceof FormData;
      const encoded = new Response(config.data);
      if (form) headers.setContentType(encoded.headers.get("content-type"));
      body = Array.from(new Uint8Array(await encoded.arrayBuffer()));
    }
    if (config.signal?.aborted) throw new CanceledError();
    config.cancelToken?.throwIfRequested();
    const request = {
      url: target.href,
      method: (config.method ?? "get").toUpperCase(),
      headers: Object.entries(headers.toJSON())
        .filter(([, value]) => value != null && value !== false)
        .map(([name, value]) => [name, String(value)]),
      body,
      timeoutMs: config.timeout || null,
    };
    let cancel: (() => void) | undefined;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    const canceled = new Promise<never>((_resolve, reject) => {
      cancel = () => reject(new CanceledError());
      config.signal?.addEventListener?.("abort", cancel);
      config.cancelToken?.promise.then(() => cancel?.());
      if (config.timeout)
        timeout = setTimeout(
          () => reject(new AxiosError("API request timed out", AxiosError.ECONNABORTED, config)),
          config.timeout
        );
    });
    try {
      const result = await Promise.race([invoke<NativeResponse>("desktop_api_request", { request }), canceled]);
      const bytes = new Uint8Array(result.body);
      const responseHeaders = new AxiosHeaders(Object.fromEntries(result.headers));
      const data =
        config.responseType === "arraybuffer"
          ? bytes.buffer
          : config.responseType === "blob"
            ? new Blob([bytes], { type: String(responseHeaders.getContentType() ?? "") })
            : new TextDecoder().decode(bytes);
      const response: AxiosResponse = {
        data,
        status: result.status,
        statusText: result.statusText,
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
