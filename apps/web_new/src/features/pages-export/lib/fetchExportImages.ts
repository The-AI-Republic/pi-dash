// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export picture loading (parity/specs/exports.md §5, EXP-018): stack
// assets resolve against the page origin and every picture is fetched to
// bytes before the document renders, so uploaded rasters embed under full
// fidelity. A picture that cannot load is skipped; the text still lands.
import { fetchBytes } from "../../../core/api/index.js";
import { platform } from "../../../core/platform/index.js";

export type FetchExportImageBytes = (url: string) => Promise<Uint8Array>;

/** Fetch one picture's bytes through the platform transport. */
export function browserFetchExportImageBytes(url: string): Promise<Uint8Array> {
  return fetchBytes(url, platform.fetch);
}

/** Decode an inline base64 picture, or null when it is not one. */
function decodeDataUrlImage(src: string): Uint8Array | null {
  const marker = ";base64,";
  const at = src.indexOf(marker);
  if (at < 0) return null;
  try {
    const binary = atob(src.slice(at + marker.length));
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return bytes;
  } catch {
    return null;
  }
}

/**
 * Load every picture, in order: inline pictures decode locally, anything
 * else resolves against the page origin and fetches. Unusable entries are
 * skipped, never fatal.
 */
export async function fetchExportImages(
  urls: readonly string[],
  origin: string,
  fetchBytes: FetchExportImageBytes
): Promise<Uint8Array[]> {
  const loaded: Uint8Array[] = [];
  for (const src of urls) {
    if (src.startsWith("data:")) {
      const decoded = decodeDataUrlImage(src);
      if (decoded !== null) loaded.push(decoded);
      continue;
    }
    let absolute: string;
    try {
      absolute = new URL(src, origin).href;
    } catch {
      continue;
    }
    try {
      loaded.push(await fetchBytes(absolute));
    } catch {
      // A missing picture never fails the export; the text still lands.
    }
  }
  return loaded;
}
