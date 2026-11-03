// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration socket address (parity/specs/pages.md §8.1, §11.6,
// PAGE-065, PAGE-068). The session dials the live mount on the app's own
// origin, so the session cookie rides the handshake and every call is
// scoped by workspace and project context in the query.

/** The live mount path behind the same-origin proxy. */
export const PAGE_LIVE_PATH = "/live/collaboration";

/** The document kind a page session asks the mount for. */
export const PAGE_LIVE_DOCUMENT_TYPE = "project_page";

/** The context one socket is scoped to. */
export interface PageLiveMount {
  workspaceSlug: string;
  projectId: string;
}

/**
 * The socket URL for one page mount: the app origin with a socket
 * scheme, the live mount path, and the document context query. Throws
 * on an origin that is not a URL.
 */
export function pageLiveSocketUrl(origin: string, mount: PageLiveMount): string {
  const url = new URL(origin);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = PAGE_LIVE_PATH;
  url.search = "";
  url.hash = "";
  url.searchParams.set("documentType", PAGE_LIVE_DOCUMENT_TYPE);
  url.searchParams.set("projectId", mount.projectId);
  url.searchParams.set("workspaceSlug", mount.workspaceSlug);
  return url.toString();
}
