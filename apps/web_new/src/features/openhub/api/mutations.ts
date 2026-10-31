// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Agent-tools preference (written from parity/specs/cloud.md §5,
// CLOUD-029). The flag lives in the catalog's own profile-settings
// namespace and defaults to off; writes apply optimistically and roll
// back with an error notice when the server refuses.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { agentToolsEnabled, updateProfile, type Profile, type ProfilePatch } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { profileQueryOptions, sessionKeys } from "../../../core/session/queries.js";
import { notifyError } from "../../../shared/shell/index.js";

/** Rollback notice: the server refused the flip, the old value is back. */
export const AGENT_TOOLS_SAVE_FAILED_TITLE = "Could not save the agent-tools preference";
export const AGENT_TOOLS_SAVE_FAILED_MESSAGE = "The previous setting was restored.";

/** Namespaced patch carrying one flag flip (CLOUD-029: builds cannot clobber each other). */
export function agentToolsPatch(enabled: boolean): ProfilePatch {
  return { settings: { openhub: { apps_enabled: enabled } } };
}

/**
 * Flip the flag inside a cached profile, preserving every sibling value.
 * Anything but a set profile reads through untouched — the write still
 * lands and the server's answered row replaces the cache.
 */
export function applyAgentToolsFlip(previous: Profile | undefined, enabled: boolean): Profile | undefined {
  if (previous === undefined) return undefined;
  const namespace = previous.settings["openhub"];
  const openhub = typeof namespace === "object" && namespace !== null ? (namespace as Record<string, unknown>) : {};
  return { ...previous, settings: { ...previous.settings, openhub: { ...openhub, apps_enabled: enabled } } };
}

/**
 * Persist one flag flip. Optimistic: the UI applies immediately, takes
 * the server's answered row on success, and rolls back with a notice
 * when the server refuses. Callers keep the control inert via
 * isPending while a write is in flight.
 */
export function useSetAgentToolsEnabled() {
  const queryClient = useQueryClient();
  return useMutation<Profile, unknown, boolean>({
    mutationFn: async (enabled: boolean) => {
      const rollback = applyOptimisticUpdate<Profile>(queryClient, sessionKeys.profile, (previous) =>
        applyAgentToolsFlip(previous, enabled)
      );
      try {
        const saved = await updateProfile(getAppClient(), agentToolsPatch(enabled));
        queryClient.setQueryData(sessionKeys.profile, saved);
        return saved;
      } catch (error) {
        rollback();
        notifyError(AGENT_TOOLS_SAVE_FAILED_TITLE, AGENT_TOOLS_SAVE_FAILED_MESSAGE);
        throw error;
      }
    },
  });
}

/**
 * The flag behind the switch: true/false once the profile loads,
 * undefined while it has not (the switch stays unmounted meanwhile).
 */
export function useAgentToolsEnabled(): boolean | undefined {
  const profile = useQuery(profileQueryOptions(getAppClient()));
  if (profile.data === undefined) return undefined;
  return agentToolsEnabled(profile.data.settings);
}
