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

import { getAgentProfile } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { BUILTIN_CHAT_ID } from "./localTransport.js";
import { unavailableReasonText } from "./approvalCopy.js";

/** The availability answer changes only with the server flag or account. */
export const LOCAL_AGENT_POLL_INTERVAL_MS = 5 * 60_000;

export interface LocalContact {
  id: string;
  name: string;
  /** Short tag beside the name. */
  label: string;
  /** Why the contact cannot be opened, or null when it can. */
  unavailableReason: string | null;
}

/** Name shown for the bundled engine in the rail and the chat header. */
export const LOCAL_CONTACT_NAME = "Built-in agent";
export const LOCAL_CONTACT_LABEL = "Local";

/** Map the server's profile answer onto what the contact shows. */
export function localContactAvailability(profile: { available: boolean; reason_code: string }): {
  unavailableReason: string | null;
} {
  return { unavailableReason: profile.available ? null : unavailableReasonText(profile.reason_code) };
}

/** True when this build can serve chat from the machine itself. */
export function hasLocalChat(): boolean {
  return platform.localChat !== undefined;
}

export function localChatQueryKey() {
  return ["runners", "local-agent", "availability"] as const;
}

/**
 * The built-in contact, or an empty list when this build has no local
 * engine. Availability is read once per mount and re-checked slowly; a
 * failure reads as unavailable with the error's own words.
 */
export function useLocalContact(): LocalContact[] {
  const enabled = hasLocalChat();
  const availability = useQuery({
    queryKey: localChatQueryKey(),
    queryFn: () => getAgentProfile(getAppClient()),
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
      error instanceof Error && error.message !== "" ? error.message : "The built-in agent is unavailable.";
  } else if (availability.data !== undefined) {
    unavailableReason = localContactAvailability(availability.data).unavailableReason;
  }
  return [
    {
      id: BUILTIN_CHAT_ID,
      name: LOCAL_CONTACT_NAME,
      label: LOCAL_CONTACT_LABEL,
      unavailableReason,
    },
  ];
}
