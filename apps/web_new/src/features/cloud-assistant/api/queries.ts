// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud assistant reads (parity/specs/cloud.md §7, CLOUD-032/037). One read
// per kind: the saved provider connections with their managed-model
// options, and the provider config the lane hook derives its state from.
// Both answers are user-level; absence reads back as the endpoint's own
// blank default rather than an error.

import { getLlmConfig, listAssistantConnections, type ApiClient } from "@pidash/api-client";
import { queryOptions, useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { cloudAssistantKeys } from "./keys.js";

/** The saved connections plus the server's managed-model and lane flags. */
export function cloudConnectionsQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: cloudAssistantKeys.connections(),
    queryFn: () => listAssistantConnections(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useCloudConnections() {
  return useQuery(cloudConnectionsQueryOptions(getAppClient()));
}

/**
 * The provider config behind the credential lane. Shared with the
 * assistant entry points so a lane change made here re-gates them on the
 * next read without another request shape.
 */
export function cloudAssistantConfigQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: cloudAssistantKeys.config(),
    queryFn: () => getLlmConfig(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useCloudAssistantConfig() {
  return useQuery(cloudAssistantConfigQueryOptions(getAppClient()));
}
