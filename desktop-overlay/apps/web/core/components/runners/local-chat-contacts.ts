/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override of the runner-picker local-contacts seam
 * (PDASHOSS01-159). Adds the bundled built-in agent as a chat contact, served
 * over local Tauri IPC (see `local-chat-transport.ts`) rather than the cloud
 * relay. Guarded by `isDesktop()` so a non-Tauri render (tests, dev web)
 * behaves like the cloud default.
 *
 * The entry is shown even when the server has the feature off
 * (`MANAGED_RUNNER_ENABLED=false` → `managed_runner_disabled`), but carries the
 * server's reason and is not clickable: offering a chat box that cannot send
 * is what made the first failure look like a hang.
 */

import useSWR from "swr";
import { agentAvailability, isDesktop } from "@/services/agent-runtime";
import { BUILTIN_RUNNER_ID } from "@/services/local-chat-transport";

export interface LocalChatContact {
  id: string;
  name: string;
  label?: string;
  unavailableReason?: string;
}

export function useLocalChatContacts(): LocalChatContact[] {
  const { data } = useSWR(isDesktop() ? "desktop-agent-availability" : null, agentAvailability, {
    revalidateOnFocus: false,
    // The answer only changes when the server flag or the account does, so
    // poll slowly rather than on every picker render.
    refreshInterval: 5 * 60 * 1000,
  });
  if (!isDesktop()) return [];
  return [
    {
      id: BUILTIN_RUNNER_ID,
      name: "Built-in agent",
      label: "Local",
      unavailableReason: data && !data.available ? data.reason : undefined,
    },
  ];
}
