// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Uploaded-file URL resolution (SHELL-061, from
// parity/specs/shell.md §4). Logo and avatar legs arrive as absolute
// URLs, data/blob URLs, or app-relative paths; relative legs resolve
// against the API base the route reads off its client. Empty legs stay
// empty so callers fall back to initials.
const ABSOLUTE_PATTERN = /^(https?:|data:|blob:)/i;

/**
 * Resolve one stored file leg to a renderable URL. Absolute, data and
 * blob legs pass through; relative legs join the API base; an empty
 * leg resolves to null.
 */
export function resolveFileUrl(path: string | null | undefined, apiBaseUrl: string): string | null {
  if (path === null || path === undefined || path === "") return null;
  if (ABSOLUTE_PATTERN.test(path)) return path;
  const base = apiBaseUrl.replace(/\/+$/, "");
  return `${base}${path.startsWith("/") ? "" : "/"}${path}`;
}
