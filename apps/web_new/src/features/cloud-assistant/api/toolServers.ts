// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tool-server registry reads and writes for the cloud page (CLOUD-036).
// The registry itself is shared product behavior; the cloud page mounts it
// because the edition replaces the whole assistant tab. Writes refresh the
// list and report through the caller's notices.

import {
  createMcpServer,
  deleteMcpServer,
  listMcpServers,
  patchMcpServer,
  type ApiClient,
  type McpServer,
  type McpServerInput,
  type McpServerPatch,
} from "@pidash/api-client";
import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { cloudAssistantKeys } from "./keys.js";

export const toolServerKeys = {
  list: () => [...cloudAssistantKeys.all, "tool-servers"] as const,
};

export function toolServersQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: toolServerKeys.list(),
    queryFn: () => listMcpServers(client),
    staleTime: LIST_STALE_TIME_MS,
  });
}

export function useToolServers() {
  return useQuery(toolServersQueryOptions(getAppClient()));
}

export function useAddToolServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: McpServerInput) => createMcpServer(getAppClient(), input),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: toolServerKeys.list() }),
  });
}

export function useUpdateToolServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { serverId: string; patch: McpServerPatch }): Promise<McpServer> =>
      patchMcpServer(getAppClient(), input.serverId, input.patch),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: toolServerKeys.list() }),
  });
}

export function useRemoveToolServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (serverId: string) => deleteMcpServer(getAppClient(), serverId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: toolServerKeys.list() }),
  });
}
