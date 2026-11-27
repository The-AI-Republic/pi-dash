// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The built-in agent as a chat contact (NEWFRONT-553, RUN-033). On the
// desktop the bundled engine appears above the connected runners; it is
// linked when the server reports the managed agent available and shown
// disabled with the server's reason otherwise, so a chat that cannot
// send never looks like a hang. Without the local platform capability
// there is no section at all, which is the cloud and web answer.

import { useQuery } from "@tanstack/react-query";

import { getPidashAgentProfile } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { hasLocalChat } from "../../../core/platform/local-chat.js";
import { BUILTIN_CHAT_ID } from "./localTransport.js";
import { agentReasonMessage } from "../../../shared/lib/agentReasons.js";
import { PIDASH_AGENT_NAME } from "../../../shared/lib/runnerIdentity.js";

/** The availability answer changes only with the server flag or account. */
export const LOCAL_AGENT_POLL_INTERVAL_MS = 5 * 60_000;

export interface PidashAgentContact {
  id: string;
  name: string;
  /** Short tag beside the name. */
  label: string;
  /** Why the contact cannot be opened, or null when it can. */
  unavailableReason: string | null;
}

export const LOCAL_CONTACT_LABEL = "Local";

/** Map the server's profile answer onto what the contact shows. */
export function pidashAgentContactAvailability(profile: { available: boolean; reason_code: string }): {
  unavailableReason: string | null;
} {
  return { unavailableReason: profile.available ? null : agentReasonMessage(profile.reason_code) };
}

export { hasLocalChat };

export function localChatQueryKey() {
  return ["runners", "local-agent", "availability"] as const;
}

/** Shown when the availability check fails without words of its own. */
export const LOCAL_AGENT_UNAVAILABLE_FALLBACK = "Pi Dash Agent is unavailable.";

/**
 * The built-in contact, or an empty list when this build has no local
 * engine. Availability is read once per mount and re-checked slowly; a
 * failure reads as unavailable with the error's own words.
 */
export function usePidashAgentContact(): PidashAgentContact[] {
  const enabled = hasLocalChat();
  const availability = useQuery({
    queryKey: localChatQueryKey(),
    queryFn: () => getPidashAgentProfile(getAppClient()),
    enabled,
    staleTime: LOCAL_AGENT_POLL_INTERVAL_MS,
    refetchInterval: LOCAL_AGENT_POLL_INTERVAL_MS,
    refetchOnWindowFocus: false,
    retry: false,
  });
  if (!enabled) return [];
  let unavailableReason: string | null = null;
  if (availability.isError) {
    const error = availability.error;
    unavailableReason =
      error instanceof Error && error.message !== "" ? error.message : LOCAL_AGENT_UNAVAILABLE_FALLBACK;
  } else if (availability.data !== undefined) {
    unavailableReason = pidashAgentContactAvailability(availability.data).unavailableReason;
  }
  return [
    {
      id: BUILTIN_CHAT_ID,
      name: PIDASH_AGENT_NAME,
      label: LOCAL_CONTACT_LABEL,
      unavailableReason,
    },
  ];
}
