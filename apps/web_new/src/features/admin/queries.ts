// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin query subtree (parity/specs/admin.md §0.3): server data for the
// god-mode console lives under its own "admin" key root — never under
// the member ["ws", ...] or ["session", ...] roots — so member
// sign-out and workspace switches neither clear nor leak it. The
// entry gate owns the bootstrap read; later slices add their queries
// under the same root.

import { queryOptions } from "@tanstack/react-query";
import { getAdminSession, getCsrfToken, getInstanceBootstrap, type ApiClient } from "@pidash/api-client";

export const adminKeys = {
  root: ["admin"],
  /** The single instance-info read the entry gate branches on. */
  bootstrap: ["admin", "instance", "bootstrap"],
  /** The forgery token the native auth POSTs carry as a hidden field. */
  csrf: ["admin", "csrf"],
  /** The admin session check the guards branch on. */
  session: ["admin", "session"],
} as const;

/**
 * The instance bootstrap the entry gate branches on (ADM-001): issued
 * exactly once — no retry, no focus or reconnect refetch — and never
 * stale, so remounts reuse it instead of re-issuing the read.
 */
export function instanceBootstrapQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.bootstrap,
    queryFn: () => getInstanceBootstrap(client),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/**
 * The admin session check the guards branch on (ADM-009): issued once
 * per page lifetime — no retry, no focus or reconnect refetch — and
 * never stale. Anonymous reads as anonymous, a revoked grant as
 * forbidden, and transient failures throw so guards fail closed.
 */
export function adminSessionQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.session,
    queryFn: () => getAdminSession(client),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/**
 * The forgery token for the native setup/sign-in POSTs. Stable for the
 * page lifetime: rotation only happens on login, which navigates away.
 */
export function adminCsrfQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.csrf,
    queryFn: () => getCsrfToken(client),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
  });
}
