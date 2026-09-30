// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// The app HTTP client (Architecture > Import rules). This module is the
// only place that builds an ApiClient: transport is platform.fetch, 401s
// expire the session store, and the CSRF token rides the default endpoint.
// Features and routes use getAppClient(); components use feature hooks.

import { createClient, type ApiClient, type Transport } from "@pidash/api-client";

import { platform, toApiTransport } from "../platform/index.js";
import { createSessionMiddleware } from "../session/middleware.js";
import { setSessionClient } from "../session/queries.js";

function defaultBaseUrl(): string {
  if (typeof globalThis.window !== "undefined" && globalThis.window.location?.origin) {
    return globalThis.window.location.origin;
  }
  return "http://localhost:8000";
}

let appClient: ApiClient | null = null;

/**
 * Build the singleton client. Called once at bootstrap; also points the
 * session hooks at the same instance so loaders and components share it.
 * Tests pass a stub transport; the app always uses platform.fetch.
 */
export function createAppClient(baseUrl: string = defaultBaseUrl(), transport?: Transport): ApiClient {
  const client = createClient({
    baseUrl,
    transport: transport ?? toApiTransport(platform),
    middleware: [createSessionMiddleware()],
  });
  appClient = client;
  setSessionClient(client);
  return client;
}

/** The bootstrap client. Throws before createAppClient has run. */
export function getAppClient(): ApiClient {
  if (!appClient) {
    throw new Error("App client is not configured yet. Call createAppClient at bootstrap.");
  }
  return appClient;
}

/** Drop the singleton. Tests only; the app never rebuilds its client. */
export function resetAppClient(): void {
  appClient = null;
}
