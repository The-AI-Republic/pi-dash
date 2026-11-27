// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Fetches an address to bytes through the platform transport (page
// exports use it for pictures and the embedded typeface; nothing here is
// API-shaped). The only feature-level byte fetching in the app.
import type { PlatformFetch } from "../platform/index.js";

/** Read the address bytes, rejecting when the answer is not found. */
export async function fetchBytes(url: string, fetchImpl: PlatformFetch): Promise<Uint8Array> {
  const response = await fetchImpl(url);
  if (!response.ok) throw new Error(`Could not load ${url}.`);
  return new Uint8Array(await response.arrayBuffer());
}

/**
 * Read the address bytes through the page transport with credentials.
 * Asset GETs answer with a redirect to presigned storage, which the
 * native API transport refuses (off-origin redirect); the page transport
 * follows it, as it already does for presigned uploads.
 */
export function fetchPageBytes(url: string): Promise<Uint8Array> {
  return fetchBytes(url, (input) => globalThis.fetch(input, { credentials: "include" }));
}
