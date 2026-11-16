// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail read (NEWFRONT-555, RUN-042). The detail page polls the
// single-runner endpoint so status and live telemetry move under the reader
// without navigation; the list cache is untouched by this read.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getRunner, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { runnersKeys } from "./keys.js";

/** Same cadence as the machines table; the detail page shares its rhythm. */
export const RUNNER_DETAIL_POLL_INTERVAL_MS = 5_000;

export function runnerDetailQueryOptions(client: ApiClient, workspaceSlug: string, runnerId: string) {
  return queryOptions({
    queryKey: runnersKeys.runnerDetail(workspaceSlug, runnerId),
    queryFn: () => getRunner(client, runnerId),
    staleTime: 0,
    refetchInterval: RUNNER_DETAIL_POLL_INTERVAL_MS,
    // A 500 renders the page's failure state; the 5 s poll is the recovery.
    retry: false,
  });
}

export function useRunnerDetail(workspaceSlug: string, runnerId: string, enabled = true) {
  return useQuery({ ...runnerDetailQueryOptions(getAppClient(), workspaceSlug, runnerId), enabled });
}
