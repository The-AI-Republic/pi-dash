/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override of the runner-picker local-contacts seam (design §11,
 * §12.1). Adds "This computer": the built-in engine of this computer's Pi Dash
 * daemon, chatted with over the local socket (see `local-chat-transport.ts`)
 * rather than the cloud relay. Guarded by `isDesktop()` so a non-Tauri render
 * (tests, dev web) behaves like the cloud default.
 *
 * The entry is shown even when the engine cannot take a chat (another account
 * on the daemon, an old daemon, the engine signed out), with the reason and
 * not clickable: a chat box that cannot send looks like a hang. No daemon yet
 * is not such a case — opening the chat offers to set one up.
 */

import useSWR from "swr";
import { agentAvailability, isDesktop } from "@/services/agent-runtime";
import { useDiscovery } from "@/components/runners/this-computer";
import { BUILTIN_RUNNER_ID } from "@/services/local-chat-transport";

export interface LocalChatContact {
  id: string;
  name: string;
  label?: string;
  unavailableReason?: string;
}

export function useLocalChatContacts(): LocalChatContact[] {
  const discovery = useDiscovery();
  const { data } = useSWR(
    isDesktop() ? ["desktop-agent-availability", discovery?.kind ?? "unknown"] : null,
    agentAvailability,
    { revalidateOnFocus: false, refreshInterval: 60 * 1000 }
  );
  if (!isDesktop()) return [];
  return [
    {
      id: BUILTIN_RUNNER_ID,
      name: "This computer",
      label: "Built-in",
      unavailableReason: data && !data.available ? data.reason : undefined,
    },
  ];
}
