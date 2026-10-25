// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Speech configuration for dictation controls (RUN-027, target per
// main#586). A composer offers dictation only when the account's speech
// endpoint is saved and the operator's switch is on, so the control has
// to know both; while the read is in flight or after it fails there is
// nothing to offer and the control stays hidden.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { getSttConfig, type ApiClient } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";

/** Cache key for the signed-in account's speech configuration. */
export const sttConfigKeys = {
  all: () => ["assistant", "stt-config"] as const,
};

export function sttConfigQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: sttConfigKeys.all(),
    queryFn: () => getSttConfig(client),
    staleTime: 60_000,
    // The operator can flip the kill switch without a web rebuild. This is
    // the one read the composer and the settings page share, so a refocus
    // is how a tab that stayed open learns the switch moved (AGT-064).
    // Other reads keep the app-wide no-focus-refetch policy.
    refetchOnWindowFocus: "always",
  });
}

/**
 * Whether a dictation control should exist. `enabled` is absent on a
 * server older than the kill switch and reads as off, exactly as the
 * switch's default. A saved key is required, so an empty setup never
 * offers a control that could only fail.
 */
export function dictationOffered(config: { enabled?: boolean | undefined; has_api_key: boolean } | undefined): boolean {
  if (config === undefined) return false;
  return config.enabled === true && config.has_api_key;
}

/** The speech configuration, or undefined while it loads or after a failure. */
export function useDictationConfig(enabled = true) {
  return useQuery({ ...sttConfigQueryOptions(getAppClient()), enabled });
}
