// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI-assistant settings reads (parity/specs/profile.md §10, PROF-021..023).
// Three reads back the tab: the model config, the dictation config and the
// tool-server registry. All are own-record only and never refreshed on a
// timer (PROF-027): the tab re-reads when it mounts, never while idle.

import { useQuery } from "@tanstack/react-query";
import { getLlmConfig, getSttConfig, listMcpServers } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { assistantSettingsKeys } from "./keys.js";

const READ_OPTIONS = {
  staleTime: REFERENCE_STALE_TIME_MS,
  refetchOnWindowFocus: false,
} as const;

export function useAssistantLlm() {
  return useQuery({
    queryKey: assistantSettingsKeys.llm,
    queryFn: () => getLlmConfig(getAppClient()),
    ...READ_OPTIONS,
  });
}

export function useAssistantVoice() {
  return useQuery({
    queryKey: assistantSettingsKeys.voice,
    queryFn: () => getSttConfig(getAppClient()),
    ...READ_OPTIONS,
  });
}

export function useAssistantServers() {
  return useQuery({
    queryKey: assistantSettingsKeys.servers,
    queryFn: () => listMcpServers(getAppClient()),
    ...READ_OPTIONS,
  });
}
