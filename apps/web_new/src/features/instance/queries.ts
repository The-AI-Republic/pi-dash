// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// App-startup instance read (parity/specs/workspace-settings.md §34-35,
// WSET-034/035). The root gate branches on the same GET /api/instances/
// envelope the auth cards read, but keeps its own key so a workspace
// switch never clears it. A failed read shows the maintenance screen at
// once and is retried in the background with exponential backoff capped
// at a short interval, so the app comes back on its own once the
// services return.
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

/** First background retry delay; each later attempt doubles up to the cap. */
export const INSTANCE_STARTUP_RETRY_MS = 250;
export const INSTANCE_STARTUP_RETRY_MAX_MS = 30_000;

/** Exponential backoff between background startup read attempts. */
export function instanceStartupRecheckDelay(errorUpdateCount: number): number {
  const attempt = Math.max(errorUpdateCount - 1, 0);
  return Math.min(INSTANCE_STARTUP_RETRY_MS * 2 ** attempt, INSTANCE_STARTUP_RETRY_MAX_MS);
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
    refetchInterval: (query) =>
      query.state.status === "error" ? instanceStartupRecheckDelay(query.state.errorUpdateCount) : false,
    retry: false,
  });
}

export function useAppInstanceBootstrap() {
  return useQuery(appInstanceQueryOptions(getAppClient()));
}

export type AppInstanceBootstrap = InstanceBootstrapState;
