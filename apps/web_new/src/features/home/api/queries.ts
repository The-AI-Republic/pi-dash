// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home queries. The assistant card reads its threads and model config
// through these hooks; components never call the HTTP client directly.
// Guests are disabled at the call site, so the card fetches nothing for
// roles that never see it.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getLlmConfig, listAssistantThreads, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { homeKeys } from "./keys.js";

/** Recent assistant threads for one workspace (SHELL-007). */
export function homeThreadsQueryOptions(client: ApiClient, workspaceSlug: string, enabled: boolean) {
  return queryOptions({
    queryKey: homeKeys.threads(workspaceSlug),
    queryFn: () => listAssistantThreads(client, workspaceSlug),
    staleTime: LIST_STALE_TIME_MS,
    enabled,
  });
}

/** Recent assistant threads for one workspace. */
export function useAssistantThreads(workspaceSlug: string, enabled: boolean) {
  return useQuery(homeThreadsQueryOptions(getAppClient(), workspaceSlug, enabled));
}

/** The user's model configuration behind the setup reminder (SHELL-007). */
export function homeAssistantConfigQueryOptions(client: ApiClient, workspaceSlug: string, enabled: boolean) {
  return queryOptions({
    queryKey: homeKeys.assistantConfig(workspaceSlug),
    queryFn: () => getLlmConfig(client),
    staleTime: REFERENCE_STALE_TIME_MS,
    enabled,
  });
}

/** The user's model configuration behind the setup reminder. */
export function useAssistantConfig(workspaceSlug: string, enabled: boolean) {
  return useQuery(homeAssistantConfigQueryOptions(getAppClient(), workspaceSlug, enabled));
}
