// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
    // Multipart wins when set; fetch derives the boundary Content-Type
    // itself. Note for the desktop bridge: a transport that re-encodes
    // the body instead of handing it to fetch must also forward that
    // derived Content-Type, or the server sees bytes it cannot parse.
    const outgoing = init.formData ?? init.body;
    if (outgoing !== undefined) {
      request.body = outgoing;
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
      // Binary legs (a pasted picture cloned by copying its bytes,
      // PAGE-039) need the body itself, not its text.
      arrayBuffer: () => response.arrayBuffer(),
    };
  };
}
