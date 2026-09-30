// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { describe, expect, it, vi } from "vitest";
import { ApiClient, createClient } from "./client.js";
import { ApiError, isApiError, isRetryable } from "./errors.js";
import type { Transport, TransportInit, TransportResponse } from "./client.js";

export interface RecordedCall {
  url: string;
  init: TransportInit;
}

export function fakeResponse(
  options: {
    status?: number;
    body?: string;
    headers?: Record<string, string>;
    url?: string;
  } = {}
): TransportResponse {
  const { status = 200, body = "", headers = {}, url = "https://api.test/x/" } = options;
  const lower: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) lower[key.toLowerCase()] = value;
  return {
    status,
    url,
    headers: { get: (name: string) => lower[name.toLowerCase()] ?? null },
    text: async () => body,
  };
}

export function recordingTransport(
  handler: (call: RecordedCall) => TransportResponse | Promise<TransportResponse>,
  calls: RecordedCall[] = []
): Transport & { calls: RecordedCall[] } {
  const transport = (async (url: string, init: TransportInit) => {
    const call = { url, init };
    calls.push(call);
    return handler(call);
  }) as Transport & { calls: RecordedCall[] };
  transport.calls = calls;
  return transport;
}

function jsonResponse(value: unknown, status = 200): TransportResponse {
  return fakeResponse({ status, body: JSON.stringify(value), headers: { "content-type": "application/json" } });
}

describe("ApiClient", () => {
  it("joins base URL and path, appends query params", async () => {
    const transport = recordingTransport(() => jsonResponse({ ok: true }));
    const client = createClient({ baseUrl: "https://api.test/", transport });
    await client.getJson("/api/users/me/", { query: { per_page: 10, cursor: undefined, q: "a b" } });
    expect(transport.calls).toHaveLength(1);
    const url = new URL(transport.calls[0]?.url ?? "");
    expect(`${url.origin}${url.pathname}`).toBe("https://api.test/api/users/me/");
    expect(url.searchParams.get("per_page")).toBe("10");
    expect(url.searchParams.get("q")).toBe("a b");
    expect(url.searchParams.has("cursor")).toBe(false);
  });

  it("sends JSON bodies and credentials", async () => {
    const transport = recordingTransport(() => jsonResponse({}));
    const client = createClient({ baseUrl: "https://api.test", transport, csrfToken: async () => "tok" });
    await client.request("POST", "/api/x/", { json: { a: 1 } });
    const call = transport.calls[0];
    expect(call?.init.method).toBe("POST");
    expect(call?.init.headers["Content-Type"]).toBe("application/json");
    expect(call?.init.body).toBe('{"a":1}');
    expect(call?.init.credentials).toBe("include");
    expect(call?.init.headers["X-CSRFTOKEN"]).toBe("tok");
  });

  it("does not send CSRF on GET by default", async () => {
    const transport = recordingTransport(() => jsonResponse({}));
    const client = createClient({ baseUrl: "https://api.test", transport });
    await client.getJson("/api/users/me/");
    expect(transport.calls[0]?.init.headers["X-CSRFTOKEN"]).toBeUndefined();
  });

  it("fetches and caches the CSRF token from the default endpoint", async () => {
    const transport = recordingTransport((call) => {
      if (call.url.endsWith("/auth/get-csrf-token/")) return jsonResponse({ csrf_token: "abc" });
      return jsonResponse({});
    });
    const client = createClient({ baseUrl: "https://api.test", transport });
    await client.request("POST", "/api/x/", { json: {} });
    await client.request("POST", "/api/y/", { json: {} });
    const csrfCalls = transport.calls.filter((call) => call.url.endsWith("/auth/get-csrf-token/"));
    expect(csrfCalls).toHaveLength(1);
    expect(transport.calls[1]?.init.headers["X-CSRFTOKEN"]).toBe("abc");
    expect(transport.calls[2]?.init.headers["X-CSRFTOKEN"]).toBe("abc");
  });

  it("normalizes auth-flow error codes", async () => {
    const transport = recordingTransport(() =>
      fakeResponse({ status: 302, body: "", url: "https://app.test/sign-in?error_code=USER_DOES_NOT_EXIST" })
    );
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await client.getJson("/api/users/me/").catch((e: unknown) => e);
    expect(isApiError(error)).toBe(true);
    expect((error as ApiError).status).toBe(302);
  });

  it("normalizes DRF detail errors", async () => {
    const transport = recordingTransport(() => jsonResponse({ detail: "Not found." }, 404));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await client.getJson("/nope/").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expect((error as ApiError).message).toBe("Not found.");
  });

  it("normalizes DRF field errors with fields attached", async () => {
    const transport = recordingTransport(() => jsonResponse({ name: ["This field is required."] }, 400));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await client.getJson("/x/").catch((e: unknown) => e);
    expect((error as ApiError).fields).toEqual({ name: ["This field is required."] });
    expect((error as ApiError).message).toContain("name");
  });

  it("maps network failures, timeouts and aborts", async () => {
    const network: Transport = async () => {
      throw new TypeError("fetch failed");
    };
    const networkError = await createClient({ baseUrl: "https://api.test", transport: network })
      .getJson("/x/")
      .catch((e: unknown) => e);
    expect((networkError as ApiError).code).toBe("network");

    const hanging: Transport = (_url, init) =>
      new Promise<TransportResponse>((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new Error("aborted")));
      });
    const timeoutError = await createClient({ baseUrl: "https://api.test", transport: hanging, timeoutMs: 20 })
      .getJson("/x/")
      .catch((e: unknown) => e);
    expect((timeoutError as ApiError).code).toBe("timeout");

    const controller = new AbortController();
    controller.abort();
    const abortError = await createClient({ baseUrl: "https://api.test", transport: hanging })
      .getJson("/x/", { signal: controller.signal })
      .catch((e: unknown) => e);
    expect((abortError as ApiError).code).toBe("aborted");
  });

  it("runs middleware in order and lets it observe the response", async () => {
    const order: string[] = [];
    const transport = recordingTransport(() => jsonResponse({}));
    const client = createClient({
      baseUrl: "https://api.test",
      transport,
      middleware: [
        async (req, next) => {
          order.push("first-in");
          const res = await next({ ...req, headers: { ...req.headers, "X-First": "1" } });
          order.push("first-out");
          return res;
        },
        async (req, next) => {
          order.push("second");
          return next(req);
        },
      ],
    });
    await client.getJson("/x/");
    expect(order).toEqual(["first-in", "second", "first-out"]);
    expect(transport.calls[0]?.init.headers["X-First"]).toBe("1");
  });

  it("rejects invalid JSON bodies as parse errors", async () => {
    const transport = recordingTransport(() => fakeResponse({ status: 200, body: "<html>" }));
    const client = createClient({ baseUrl: "https://api.test", transport });
    const error = await client.getJson("/x/").catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("parse");
  });

  it("turns schema violations into contract errors, skipped when disabled", async () => {
    const { z } = await import("zod/mini");
    const schema = z.object({ id: z.string() });
    const client = createClient({ baseUrl: "https://api.test", transport: recordingTransport(() => jsonResponse({})) });
    const error = await (async () => client.parse(schema, { nope: 1 }, "thing"))().catch((e: unknown) => e);
    expect((error as ApiError).code).toBe("contract");

    const lax = createClient({
      baseUrl: "https://api.test",
      transport: recordingTransport(() => jsonResponse({})),
      validate: false,
    });
    expect(lax.parse(schema, { nope: 1 }, "thing")).toEqual({ nope: 1 });
  });

  it("marks retryable failures", () => {
    expect(isRetryable(new ApiError({ code: "network", message: "x" }))).toBe(true);
    expect(isRetryable(new ApiError({ status: 503, code: "http", message: "x" }))).toBe(true);
    expect(isRetryable(new ApiError({ status: 404, code: "http", message: "x" }))).toBe(false);
    expect(isRetryable(new Error("plain"))).toBe(false);
  });

  it("exposes the validate flag", () => {
    expect(new ApiClient({ baseUrl: "https://api.test" }).validates).toBe(true);
    expect(new ApiClient({ baseUrl: "https://api.test", validate: false }).validates).toBe(false);
  });

  it("calls through to a fetch-compatible function", async () => {
    const fetchLike = vi.fn(async () => fakeResponse({ status: 200, body: '{"a":1}' }));
    const client = createClient({ baseUrl: "https://api.test", transport: fetchLike as unknown as Transport });
    await expect(client.getJson("/x/")).resolves.toEqual({ a: 1 });
    expect(fetchLike).toHaveBeenCalledOnce();
  });
});
