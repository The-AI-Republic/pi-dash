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
import {
  getAdminSession,
  getCsrfToken,
  getInstanceBootstrap,
  listInstanceAdmins,
  probeCloudAuthSession,
  type ApiClient,
} from "@pidash/api-client";

export const adminKeys = {
  root: ["admin"],
  /** The single instance-info read the entry gate branches on. */
  bootstrap: ["admin", "instance", "bootstrap"],
  /** The admins list the general page reads its first email from. */
  admins: ["admin", "instance", "admins"],
  /** The forgery token the native auth POSTs carry as a hidden field. */
  csrf: ["admin", "csrf"],
  /** The admin session check the guards branch on. */
  session: ["admin", "session"],
  /** The cloud entry's one-shot auth probe (cloud only). */
  cloudProbe: ["admin", "cloud", "probe"],
} as const;

/**
 * Shared fetch behavior every admin query spreads (ADM-069, from
 * parity/specs/admin.md §0.6): entries never go stale, so
 * client-side hops reuse the cache instead of re-issuing reads;
 * window focus and reconnects never refetch; and no polling interval
 * exists. Lists refresh only through explicit invalidation after a
 * mutation. Mount refetch stays on by design: an invalidated entry
 * must re-read when its page mounts again.
 */
export const adminQueryDefaults = {
  staleTime: Number.POSITIVE_INFINITY,
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  refetchInterval: false,
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
    ...adminQueryDefaults,
    retry: false,
  });
}

/**
 * The instance admins list (ADM-014): read once per page lifetime —
 * no retry, no focus or reconnect refetch — and never stale. A failed
 * read surfaces through the page's error block with a retry.
 */
export function instanceAdminsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.admins,
    queryFn: () => listInstanceAdmins(client),
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
    ...adminQueryDefaults,
    retry: false,
  });
}

/**
 * The cloud entry's one-shot auth probe (ADM-079): classifies a genuine
 * admin-check 403 into authenticated, anonymous, or inconclusive. The
 * hook enables it only on that verdict, so it fires exactly there —
 * never on success and never on a transient check failure. One shot
 * like the check: no retry, no focus or reconnect refetch, never
 * stale. Retry resets it alongside the check so a fresh 403 re-probes.
 */
export function cloudAuthProbeQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.cloudProbe,
    queryFn: () => probeCloudAuthSession(client),
    ...adminQueryDefaults,
    retry: false,
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
    ...adminQueryDefaults,
  });
}
