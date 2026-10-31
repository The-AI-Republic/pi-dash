// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin query subtree (parity/specs/admin.md §0.3): server data for the
// god-mode console lives under its own "admin" key root — never under
// the member ["ws", ...] or ["session", ...] roots — so member
// sign-out and workspace switches neither clear nor leak it. The
// entry gate owns the bootstrap read; later slices add their queries
// under the same root.

import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import {
  getAdminSession,
  getCsrfToken,
  getInstanceBootstrap,
  getInstanceBootstrapForStartup,
  getLoopJob,
  listInstanceAdmins,
  listInstanceConfigurations,
  listInstanceRunners,
  listInstanceWorkspaces,
  listLoopJobs,
  listLoopTargets,
  probeCloudAuthSession,
  type AdminWorkspaceListResponse,
  type ApiClient,
  type InstanceConfigurationList,
} from "@pidash/api-client";

export const adminKeys = {
  root: ["admin"],
  /** The entry gate's one-shot instance read (unprovisioned-safe). */
  entryBootstrap: ["admin", "instance", "entry"],
  /** The single instance-info read the dashboard pages reuse. */
  bootstrap: ["admin", "instance", "bootstrap"],
  /** The admins list the general page reads its first email from. */
  admins: ["admin", "instance", "admins"],
  /** The forgery token the native auth POSTs carry as a hidden field. */
  csrf: ["admin", "csrf"],
  /** The admin session check the guards branch on. */
  session: ["admin", "session"],
  /** The cloud entry's one-shot auth probe (cloud only). */
  cloudProbe: ["admin", "cloud", "probe"],
  /** The shared configurations key map every settings page reads. */
  configurations: ["admin", "configurations"],
  /** The loop jobs list the loop pages read and revalidate. */
  loopJobs: ["admin", "loop", "jobs"],
  /**
   * The one-job read the loop detail page renders (ADM-056/058). The
   * edit modal already invalidates it on settle; the detail slice
   * (NEWFRONT-517) hangs its query off this key.
   */
  loopJobDetail: (jobId: string) => ["admin", "loop", "job", jobId],
  /**
   * One job's targets read (ADM-058/059). The key carries the skip
   * filter ("" for all), so each filter value keeps its own cached
   * entry and resetting to all reuses the cache instead of
   * refetching. A sibling of the detail key rather than a child, so
   * editing the job revalidates the job without re-reading targets.
   */
  loopJobTargets: (jobId: string, skipReason: string) => ["admin", "loop", "targets", jobId, skipReason],
  /** The cursor-paginated instance workspace list. */
  workspaces: ["admin", "workspaces"],
  /**
   * One instance runners page (ADM-080). The key carries the
   * provisioning filter ("" for all) and the page number, so each
   * combination keeps its own cached entry and returning to it
   * reuses the cache instead of refetching.
   */
  runners: (provisioning: string, page: number) => ["admin", "runners", provisioning, page],
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
export function instanceEntryQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.entryBootstrap,
    // A fresh install answers only the bare setup flags; the startup
    // reader folds that envelope into "setup unfinished" while still
    // validating every provisioned answer against the full contract.
    queryFn: () => getInstanceBootstrapForStartup(client),
    ...adminQueryDefaults,
    retry: false,
  });
}

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
    // A failed check is a verdict, not a cache miss: the error screen
    // and its retry own the next read. Without this, every remount of
    // a consumer (the guard and the cloud resolver both mount it)
    // re-issued the read, so an outage spun refetch → render → refetch
    // forever and never settled on the retry screen (ADM-078).
    retryOnMount: false,
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
    // One shot, like the check: a settled probe is the classification
    // the gate branches on, so a remount never re-probes behind it.
    retryOnMount: false,
  });
}

/**
 * The next workspace-list cursor (ADM-040): the page reporting further
 * results carries the cursor to fetch; anything else ends the list. An
 * empty cursor reads as no further page — the list read treats it as
 * the first page, so following it would re-read what is shown.
 */
export function selectNextWorkspaceCursor(lastPage: AdminWorkspaceListResponse): string | undefined {
  if (!lastPage.next_page_results || lastPage.next_cursor === "") return undefined;
  return lastPage.next_cursor;
}

/**
 * The configurations key map every settings page shares (ADM-019/022,
 * ADM-024/036, ADM-046, ADM-069): one fetch — never stale, never
 * refetched behind the pages' backs. Saves merge the answered rows
 * back in (see mergeConfigurationRows) instead of refetching; only an
 * explicit mutation-side revalidation refreshes it.
 */
export function instanceConfigurationsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.configurations,
    queryFn: () => listInstanceConfigurations(client),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/**
 * The loop jobs list (ADM-051): one cached read per page lifetime —
 * no retry, no focus or reconnect refetch, never stale — refreshed
 * only by explicit revalidation after a mutation (spec §0.6). Every
 * loop mutation invalidates this key on settle.
 */
export function loopJobsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: adminKeys.loopJobs,
    queryFn: () => listLoopJobs(client),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });
}

/**
 * One loop job read with its stats rollup (ADM-058): the shared
 * never-stale shape, refreshed only by explicit revalidation after
 * an edit (spec §0.6).
 */
export function loopJobDetailQueryOptions(client: ApiClient, jobId: string) {
  return queryOptions({
    queryKey: adminKeys.loopJobDetail(jobId),
    queryFn: () => getLoopJob(client, jobId),
    ...adminQueryDefaults,
    retry: false,
  });
}

/**
 * One job's targets read (ADM-058/059): the shared never-stale shape
 * per filter value, so picking a reason fetches its entry once and
 * resetting to all serves the cached unfiltered entry with no
 * refetch. The blank filter passes through; the contract drops the
 * empty skip_reason from the URL.
 */
export function loopJobTargetsQueryOptions(client: ApiClient, jobId: string, skipReason: string) {
  return queryOptions({
    queryKey: adminKeys.loopJobTargets(jobId, skipReason),
    queryFn: () => listLoopTargets(client, jobId, { skipReason }),
    ...adminQueryDefaults,
    retry: false,
  });
}

/**
 * Fold answered rows into the cached key map by key, so a save re-seeds
 * the pages from stored state without a refetch. Rows the server did
 * not answer keep their cached values; answered rows for unknown keys
 * append; an empty cache starts from the answer alone.
 */
export function mergeConfigurationRows(
  current: InstanceConfigurationList | undefined,
  updated: InstanceConfigurationList
): InstanceConfigurationList {
  if (current === undefined) return [...updated];
  const answered = new Map(updated.map((row) => [row.key, row]));
  const merged = current.map((row) => answered.get(row.key) ?? row);
  for (const row of updated) {
    if (!merged.some((existing) => existing.key === row.key)) merged.push(row);
  }
  return merged;
}

/**
 * The instance workspace list (ADM-038, ADM-040): cursor-paginated, ten
 * per page. Pages append; nothing already shown is refetched. Same
 * single-shot caching as the other admin reads.
 */
export function instanceWorkspacesQueryOptions(client: ApiClient) {
  return infiniteQueryOptions({
    queryKey: adminKeys.workspaces,
    queryFn: ({ pageParam }: { pageParam: string | undefined }) => listInstanceWorkspaces(client, pageParam),
    getNextPageParam: selectNextWorkspaceCursor,
    initialPageParam: undefined as string | undefined,
    ...adminQueryDefaults,
    retry: false,
  });
}

/**
 * One instance runners page (ADM-080): the shared never-stale shape
 * per filter value and page, so paging and filtering fetch each
 * entry once and reuse it afterwards. The blank filter passes
 * through; the contract drops the empty provisioning from the URL.
 */
export function instanceRunnersQueryOptions(client: ApiClient, provisioning: string, page: number) {
  return queryOptions({
    queryKey: adminKeys.runners(provisioning, page),
    queryFn: () => listInstanceRunners(client, { provisioning, page }),
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
