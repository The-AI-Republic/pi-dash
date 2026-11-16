// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared OpenHub stub harness for the Apps catalog oracle scenarios
// (NEWFRONT-439, CLOUD-021–031). No gateway answers behind the scratch
// stack, so Hub-backed payloads (a connected status, catalog pages,
// install/auth answers) are fulfilled at the browser boundary while the
// real old-app bundle renders them; everything Django answers itself
// (status-disconnected shape, taxonomy failures, begin redirects,
// api-key/icon validation, profile reads) is asserted live instead.
// One dispatcher route covers the whole context — the Apps page and any
// OAuth popup it opens — and records every OpenHub request for
// ordering/payload assertions. Unrouted catalog calls fail safe with a
// 503 (never the live 401, which the app's session guard would bounce
// to sign-in); the OAuth begin continues live unless a spec overrides
// it, so the handoff proof exercises the real server redirect.
import type { BrowserContext, Page, Route } from "@playwright/test";

/** One OpenHub request the dispatcher saw, in arrival order. */
export interface OpenhubLoggedRequest {
  method: string;
  /** Path plus query string (origin stripped). */
  path: string;
  /** Raw POST body, or null for bodyless requests. */
  postData: string | null;
  /** Request Cookie header, so per-user stubs can tell callers apart. */
  cookie: string;
}

/** What a per-endpoint handler resolves with. */
export type OpenhubStubAnswer = { status: number; body?: unknown; contentType?: string } | "continue" | "abort";

export type OpenhubStubHandler = (
  route: Route,
  logged: OpenhubLoggedRequest
) => Promise<OpenhubStubAnswer> | OpenhubStubAnswer;

export interface OpenhubStubOptions {
  status?: OpenhubStubHandler;
  marketplace?: OpenhubStubHandler;
  installations?: OpenhubStubHandler;
  install?: OpenhubStubHandler;
  authStatus?: OpenhubStubHandler;
  apiKey?: OpenhubStubHandler;
  icon?: OpenhubStubHandler;
  oauthBegin?: OpenhubStubHandler;
  /** Shared request log; a fresh array when omitted. */
  log?: OpenhubLoggedRequest[];
}

export interface InstalledOpenhubStub {
  log: OpenhubLoggedRequest[];
  uninstall: () => Promise<void>;
}

const PATTERN = "**/api/openhub/**";

function json(status: number, body: unknown): OpenhubStubAnswer {
  return { status, body };
}

function unavailable(): OpenhubStubAnswer {
  return json(503, {
    error: "openhub_unavailable",
    detail: "OpenHub is unavailable right now.",
  });
}

/** Connected status payload; the hub base defaults to the scratch one. */
export function connectedStatus(hubUrl = "https://hub.airepublic.com"): Record<string, unknown> {
  return {
    connected: true,
    has_gateway_scopes: true,
    scopes: ["chat", "apps", "models"],
    credential_type: "oidc",
    hub_url: hubUrl,
    wallet_url: `${hubUrl.replace(/\/+$/, "")}/account/credits`,
  };
}

/** Catalog page payload. */
export function marketplacePage(items: unknown[], nextCursor: string | null = null): Record<string, unknown> {
  return { items, nextCursor, totalCount: items.length, userStateAvailable: true };
}

/** Minimal catalog app row; specs override per-field. */
export function stubApp(overrides: Record<string, unknown> & { appId: string; name: string }): Record<string, unknown> {
  return {
    slug: overrides["appId"],
    description: `${overrides["name"]} description`,
    authType: "none",
    isInstalled: false,
    suggestedAction: "install",
    ...overrides,
  };
}

export async function installOpenhubStub(
  target: Page | BrowserContext,
  options: OpenhubStubOptions = {}
): Promise<InstalledOpenhubStub> {
  const log = options.log ?? [];
  const handlers: OpenhubStubHandler = async (route, logged) => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const pick = (name: keyof OpenhubStubOptions): OpenhubStubHandler | undefined => {
      const handler = options[name];
      return typeof handler === "function" ? (handler as OpenhubStubHandler) : undefined;
    };
    if (path === "/api/openhub/status/") {
      const handler = pick("status") ?? (() => json(200, connectedStatus()));
      return handler(route, logged);
    }
    if (path === "/api/openhub/apps/marketplace/") {
      const handler = pick("marketplace") ?? (() => json(200, marketplacePage([])));
      return handler(route, logged);
    }
    if (path === "/api/openhub/apps/installations/") {
      const handler = pick("installations") ?? (() => json(200, marketplacePage([])));
      return handler(route, logged);
    }
    if (path === "/api/openhub/apps/icon/") {
      const handler = pick("icon") ?? "continue";
      return typeof handler === "function" ? handler(route, logged) : handler;
    }
    const begin = /^\/api\/openhub\/apps\/[^/]+\/auth\/oauth\/begin\/$/.exec(path);
    if (begin !== null) {
      const handler = pick("oauthBegin") ?? "continue";
      return typeof handler === "function" ? handler(route, logged) : handler;
    }
    if (/^\/api\/openhub\/apps\/[^/]+\/install\/$/.test(path)) {
      const handler = pick("install") ?? (() => unavailable());
      return handler(route, logged);
    }
    if (/^\/api\/openhub\/apps\/[^/]+\/auth\/status\/$/.test(path)) {
      const handler = pick("authStatus") ?? (() => unavailable());
      return handler(route, logged);
    }
    if (/^\/api\/openhub\/apps\/[^/]+\/auth\/api-key\/$/.test(path)) {
      const handler = pick("apiKey") ?? (() => unavailable());
      return handler(route, logged);
    }
    return "continue";
  };

  const dispatch = async (route: Route): Promise<void> => {
    const request = route.request();
    const url = new URL(request.url());
    const logged: OpenhubLoggedRequest = {
      method: request.method(),
      path: `${url.pathname}${url.search}`,
      postData: request.postData(),
      cookie: (await request.headerValue("cookie").catch(() => null)) ?? "",
    };
    log.push(logged);
    const answer = await handlers(route, logged);
    if (answer === "continue") return route.continue();
    if (answer === "abort") return route.abort();
    return route.fulfill({
      status: answer.status,
      contentType: answer.contentType ?? "application/json",
      body: typeof answer.body === "string" ? answer.body : JSON.stringify(answer.body ?? {}),
    });
  };

  await target.route(PATTERN, dispatch);
  return {
    log,
    uninstall: async () => {
      await target.unroute(PATTERN, dispatch).catch(() => undefined);
    },
  };
}
