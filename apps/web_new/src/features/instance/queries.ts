// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App-startup instance read (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). The root gate branches on the same GET /api/instances/
// envelope the auth cards read, but keeps its own key so a workspace
// switch never clears it. A failed read gets a bounded set of retries
// with backoff, then keeps rechecking on an interval while it is still
// failing, so the app comes back on its own once the services return.
//
// The backend answers an unprovisioned instance with only
// `{is_activated: false, is_setup_done: false}` (no config or row), so the
// startup reader accepts that exact envelope as "not ready" while still
// validating every provisioned answer against the shared contract.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getInstanceBootstrapForStartup, type ApiClient, type InstanceBootstrapState } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";

export const instanceAppKeys = {
  bootstrap: ["instance", "app", "bootstrap"],
} as const;

/** Startup read retries before the maintenance screen takes over. */
export const INSTANCE_STARTUP_RETRIES = 3;
/** First retry delay; each later attempt doubles up to the cap. */
export const INSTANCE_STARTUP_RETRY_MS = 250;
export const INSTANCE_STARTUP_RETRY_MAX_MS = 30_000;
/** How often the maintenance screen asks again after the retries run out. */
export const INSTANCE_STARTUP_RECHECK_MS = 30_000;

/** Exponential backoff between startup read attempts. */
export function instanceStartupRetryDelay(attemptIndex: number): number {
  return Math.min(INSTANCE_STARTUP_RETRY_MS * 2 ** attemptIndex, INSTANCE_STARTUP_RETRY_MAX_MS);
}

/** The desktop cache may keep everything except the one-shot startup answer. */
export function persistAppCache(queryKey: readonly unknown[]): boolean {
  return (
    queryKey.length !== instanceAppKeys.bootstrap.length ||
    queryKey.some((part, index) => part !== instanceAppKeys.bootstrap[index])
  );
}

/** The one instance-info read the root gate branches on. */
export function appInstanceQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: instanceAppKeys.bootstrap,
    queryFn: () => getInstanceBootstrapForStartup(client),
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    refetchInterval: (query) => (query.state.status === "error" ? INSTANCE_STARTUP_RECHECK_MS : false),
    retry: INSTANCE_STARTUP_RETRIES,
    retryDelay: instanceStartupRetryDelay,
  });
}

export function useAppInstanceBootstrap() {
  return useQuery(appInstanceQueryOptions(getAppClient()));
}

export type AppInstanceBootstrap = InstanceBootstrapState;
