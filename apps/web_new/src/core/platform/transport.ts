// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Adapts the active platform transport to the api-client Transport shape
// so the HTTP client never touches globals or the build target directly.

import type { Transport } from "@pidash/api-client";

import type { Platform } from "./types.js";

function headerProxy(headers: Headers): { get(name: string): string | null } {
  return { get: (name) => headers.get(name) };
}

/** Build the injectable transport the ApiClient sends every request through. */
export function toApiTransport(platform: Platform): Transport {
  return async (url, init) => {
    const request: RequestInit = { method: init.method, headers: init.headers };
    if (init.body !== undefined) {
      request.body = init.body;
    }
    if (init.signal !== undefined) {
      request.signal = init.signal;
    }
    if (init.credentials !== undefined) {
      request.credentials = init.credentials;
    }
    const response = await platform.fetch(url, request);
    return {
      status: response.status,
      url: response.url,
      headers: headerProxy(response.headers),
      text: () => response.text(),
    };
  };
}
