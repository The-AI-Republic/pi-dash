// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Small fetch client: base URL, cookies, CSRF, JSON, AbortSignal, timeout,
// normalized errors, middleware, injectable transport.
import { ApiError } from "./errors.js";

export type HttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

/** Minimal surface the client needs; satisfied by fetch and by test fakes. */
export interface Transport {
  (url: string, init: TransportInit): Promise<TransportResponse>;
}

export interface TransportInit {
  method: HttpMethod;
  headers: Record<string, string>;
  body?: string | undefined;
  signal?: AbortSignal | undefined;
  credentials?: "include" | "same-origin" | "omit";
}

export interface TransportResponse {
  status: number;
  url: string;
  headers: { get(name: string): string | null };
  text(): Promise<string>;
}

/**
 * A fetch-compatible function satisfies Transport directly: RequestInit
 * accepts this init shape, and Response carries status, url, headers and
 * text(). Pass `fetch` (or a stub with the same surface) as-is.
 */

export interface MiddlewareContext {
  method: HttpMethod;
  url: string;
  headers: Record<string, string>;
  body?: string | undefined;
}

export type Middleware = (
  request: MiddlewareContext,
  next: (request: MiddlewareContext) => Promise<TransportResponse>
) => Promise<TransportResponse>;

export interface ClientOptions {
  baseUrl: string;
  transport?: Transport;
  timeoutMs?: number;
  /** Resolve a CSRF token for unsafe methods; defaults to the auth endpoint. */
  csrfToken?: () => Promise<string | null>;
  middleware?: Middleware[];
  /**
   * Override schema validation. Defaults to validating whenever
   * NODE_ENV is not "production".
   */
  validate?: boolean;
}

export interface RequestOptions {
  query?: Record<string, string | number | boolean | undefined | null>;
  json?: unknown;
  form?: Record<string, string | undefined | null>;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  timeoutMs?: number;
  /** Send the CSRF header; defaults to true for unsafe methods. */
  csrf?: boolean;
}

const UNSAFE_METHODS: ReadonlySet<HttpMethod> = new Set(["POST", "PUT", "PATCH", "DELETE"]);

export const CSRF_HEADER = "X-CSRFTOKEN";
export const CSRF_ENDPOINT = "/auth/get-csrf-token/";
const DEFAULT_TIMEOUT_MS = 30_000;

export interface ParsedEnvelope<T> {
  data: T;
  status: number;
  url: string;
}

export class ApiClient {
  readonly baseUrl: string;
  private readonly transport: Transport;
  private readonly timeoutMs: number;
  private readonly csrfToken: (() => Promise<string | null>) | undefined;
  private readonly middleware: Middleware[];
  private readonly validateSchemas: boolean;
  private csrfCache: Promise<string | null> | null = null;

  constructor(options: ClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.transport = options.transport ?? (globalThis.fetch as Transport);
    this.timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    this.csrfToken = options.csrfToken;
    this.middleware = options.middleware ?? [];
    this.validateSchemas =
      options.validate ?? (typeof process === "undefined" || process.env.NODE_ENV !== "production");
  }

  /** Whether response schemas are validated on this client. */
  get validates(): boolean {
    return this.validateSchemas;
  }

  clearCsrfCache(): void {
    this.csrfCache = null;
  }

  private resolveCsrfToken(): Promise<string | null> {
    if (this.csrfToken) return this.csrfToken();
    if (!this.csrfCache) {
      this.csrfCache = this.getJson<{ csrf_token: string }>(CSRF_ENDPOINT, { csrf: false }).then(
        (body) => (typeof body.csrf_token === "string" ? body.csrf_token : null),
        () => null
      );
    }
    return this.csrfCache;
  }

  async request(method: HttpMethod, path: string, options: RequestOptions = {}): Promise<TransportResponse> {
    const headers: Record<string, string> = { ...(options.headers ?? {}) };
    let body: string | undefined;
    if (options.json !== undefined) {
      headers["Content-Type"] = headers["Content-Type"] ?? "application/json";
      body = JSON.stringify(options.json);
    } else if (options.form !== undefined) {
      headers["Content-Type"] = headers["Content-Type"] ?? "application/x-www-form-urlencoded";
      const params = new URLSearchParams();
      for (const [key, value] of Object.entries(options.form)) {
        if (value !== undefined && value !== null) params.append(key, value);
      }
      body = params.toString();
    }

    const needsCsrf = options.csrf ?? UNSAFE_METHODS.has(method);
    if (needsCsrf && !headers[CSRF_HEADER]) {
      const token = await this.resolveCsrfToken();
      if (token) headers[CSRF_HEADER] = token;
    }

    const url = this.buildUrl(path, options.query);
    const timeoutMs = options.timeoutMs ?? this.timeoutMs;
    const { signal, cancelTimeout } = this.raceTimeout(timeoutMs, options.signal);

    const initial: MiddlewareContext = { method, url, headers, body };
    const dispatch = (request: MiddlewareContext, index: number): Promise<TransportResponse> => {
      if (index >= this.middleware.length) {
        return this.transport(request.url, {
          method: request.method,
          headers: request.headers,
          body: request.body,
          signal: signal ?? undefined,
          credentials: "include",
        });
      }
      const layer = this.middleware[index];
      if (!layer) return dispatch(request, index + 1);
      return layer(request, (nextRequest) => dispatch(nextRequest, index + 1));
    };

    if (signal?.aborted) {
      cancelTimeout();
      throw this.toTransportError(new Error("Request was aborted"), signal, timeoutMs);
    }

    try {
      const response = await dispatch(initial, 0);
      if (response.status >= 200 && response.status < 300) return response;
      throw await this.toHttpError(response);
    } catch (error) {
      throw this.toTransportError(error, signal, timeoutMs);
    } finally {
      cancelTimeout();
    }
  }

  async getJson<T>(path: string, options: RequestOptions = {}): Promise<T> {
    const response = await this.request("GET", path, options);
    return this.readJson<T>(response);
  }

  async sendJson<T>(method: HttpMethod, path: string, options: RequestOptions = {}): Promise<ParsedEnvelope<T>> {
    const response = await this.request(method, path, options);
    return { data: await this.readJson<T>(response), status: response.status, url: response.url };
  }

  async get(path: string, options: RequestOptions = {}): Promise<TransportResponse> {
    return this.request("GET", path, options);
  }

  async post(path: string, options: RequestOptions = {}): Promise<TransportResponse> {
    return this.request("POST", path, options);
  }

  async put(path: string, options: RequestOptions = {}): Promise<TransportResponse> {
    return this.request("PUT", path, options);
  }

  async patch(path: string, options: RequestOptions = {}): Promise<TransportResponse> {
    return this.request("PATCH", path, options);
  }

  async delete(path: string, options: RequestOptions = {}): Promise<TransportResponse> {
    return this.request("DELETE", path, options);
  }

  /**
   * Validate `data` against a zod-mini schema. Active in development and
   * tests; skipped in production builds so validation never costs bytes or
   * cycles there. Violations become ApiErrors with code "contract".
   */
  parse<T>(schema: { parse(data: unknown): T }, data: unknown, what: string): T {
    if (!this.validateSchemas) return data as T;
    try {
      return schema.parse(data);
    } catch (error) {
      throw new ApiError({
        status: 0,
        code: "contract",
        message: `${what} did not match its contract`,
        cause: error,
      });
    }
  }

  private buildUrl(path: string, query?: RequestOptions["query"]): string {
    const absolute = /^https?:\/\//i.test(path) ? path : `${this.baseUrl}${path.startsWith("/") ? path : `/${path}`}`;
    if (!query) return absolute;
    const url = new URL(absolute);
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === null) continue;
      url.searchParams.append(key, String(value));
    }
    return url.toString();
  }

  private raceTimeout(
    timeoutMs: number,
    external?: AbortSignal
  ): { signal: AbortSignal | null; cancelTimeout: () => void } {
    if (external?.aborted) {
      const signal = external;
      return { signal, cancelTimeout: () => {} };
    }
    const controller = new AbortController();
    const onExternalAbort = (): void => controller.abort(external?.reason);
    external?.addEventListener("abort", onExternalAbort, { once: true });
    let timer: ReturnType<typeof setTimeout> | undefined;
    let timedOut = false;
    if (timeoutMs > 0 && timeoutMs !== Number.POSITIVE_INFINITY) {
      timer = setTimeout(() => {
        timedOut = true;
        controller.abort(new Error(`request timed out after ${timeoutMs}ms`));
      }, timeoutMs);
      if (typeof timer === "object" && typeof (timer as { unref?: () => void }).unref === "function") {
        (timer as unknown as { unref: () => void }).unref();
      }
    }
    const signal = controller.signal;
    Object.defineProperty(signal, "__apiClientTimedOut", { value: () => timedOut, configurable: true });
    return {
      signal,
      cancelTimeout: () => {
        if (timer !== undefined) clearTimeout(timer);
        external?.removeEventListener("abort", onExternalAbort);
      },
    };
  }

  private async readJson<T>(response: TransportResponse): Promise<T> {
    const text = await response.text();
    if (!text) return undefined as T;
    try {
      return JSON.parse(text) as T;
    } catch (error) {
      throw new ApiError({
        status: response.status,
        code: "parse",
        message: "Response was not valid JSON",
        cause: error,
      });
    }
  }

  private async toHttpError(response: TransportResponse): Promise<ApiError> {
    const text = await response.text().catch(() => "");
    let body: unknown = null;
    if (text) {
      try {
        body = JSON.parse(text);
      } catch {
        body = null;
      }
    }
    if (body !== null && typeof body === "object") {
      const record = body as Record<string, unknown>;
      if (typeof record["error_code"] === "string") {
        return new ApiError({
          status: response.status,
          code: record["error_code"],
          message: typeof record["error_message"] === "string" ? record["error_message"] : "Request failed",
        });
      }
      if (typeof record["detail"] === "string") {
        const code = record["code"];
        return new ApiError({
          status: response.status,
          code: typeof code === "string" ? code : "http",
          message: record["detail"],
        });
      }
      if (
        typeof (record as { code?: unknown }).code === "string" &&
        typeof (record as { message?: unknown }).message === "string"
      ) {
        return new ApiError({
          status: response.status,
          code: (record as { code: string }).code,
          message: (record as { message: string }).message,
        });
      }
      const fields: Record<string, string[]> = {};
      let hasFields = false;
      for (const [key, value] of Object.entries(record)) {
        if (Array.isArray(value) && value.every((item) => typeof item === "string")) {
          fields[key] = value as string[];
          hasFields = true;
        }
      }
      if (hasFields) {
        let message = "Request failed";
        for (const [field, messages] of Object.entries(fields)) {
          message = `${field}: ${messages.join(", ")}`;
          break;
        }
        return new ApiError({ status: response.status, code: "http", message, fields });
      }
    }
    return new ApiError({
      status: response.status,
      code: "http",
      message: text && text.length < 300 ? text : `Request failed with status ${response.status}`,
    });
  }

  private toTransportError(error: unknown, signal: AbortSignal | null, timeoutMs: number): unknown {
    if (error instanceof ApiError) return error;
    if (signal?.aborted) {
      const timedOut =
        (signal as AbortSignal & { __apiClientTimedOut?: () => boolean }).__apiClientTimedOut?.() ?? false;
      if (!timedOut) {
        return new ApiError({ status: 0, code: "aborted", message: "Request was aborted", cause: error });
      }
      return new ApiError({
        status: 0,
        code: "timeout",
        message: `Request timed out after ${timeoutMs}ms`,
        cause: error,
      });
    }
    if (error instanceof Error && /timed out/i.test(error.message)) {
      return new ApiError({ status: 0, code: "timeout", message: error.message, cause: error });
    }
    return new ApiError({
      status: 0,
      code: "network",
      message: error instanceof Error ? error.message : "Network request failed",
      cause: error,
    });
  }
}

export function createClient(options: ClientOptions): ApiClient {
  return new ApiClient(options);
}
