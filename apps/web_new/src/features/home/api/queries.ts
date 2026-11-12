// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home queries. The assistant card reads its threads and model config
// through these hooks; components never call the HTTP client directly.
// Guests are disabled at the call site, so the card fetches nothing for
// roles that never see it.

import { useQuery } from "@tanstack/react-query";
import type { ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { assistantConfigQueryOptions, assistantThreadsQueryOptions } from "../../assistant/index.js";

/**
 * Recent assistant threads for one workspace (SHELL-007). The read and
 * its cache key belong to the assistant feature so the dashboard card and
 * the assistant surfaces share one entry.
 */
export function homeThreadsQueryOptions(client: ApiClient, workspaceSlug: string, enabled: boolean) {
  return assistantThreadsQueryOptions(client, workspaceSlug, enabled);
}

/** Recent assistant threads for one workspace. */
export function useAssistantThreads(workspaceSlug: string, enabled: boolean) {
  return useQuery(homeThreadsQueryOptions(getAppClient(), workspaceSlug, enabled));
}

/** The user's model configuration behind the setup reminder (SHELL-007). */
export function homeAssistantConfigQueryOptions(client: ApiClient, workspaceSlug: string) {
  return assistantConfigQueryOptions(client, workspaceSlug);
}

/** The user's model configuration behind the setup reminder. */
export function useAssistantConfig(workspaceSlug: string, enabled: boolean) {
  // Disabled callers (guests) still mount the hook; the query stays idle.
  return useQuery({
    ...homeAssistantConfigQueryOptions(getAppClient(), workspaceSlug),
    enabled,
  });
}
