// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile writes, split out of api.ts (NEWFRONT-827, size gate). The gated
// shell stamps the last workspace on every first paint, so this hook rides
// the initial-JS chain; api.ts additionally serves the tour, invitation and
// workspace-creation hooks, whose contracts must stay in lazy chunks. Keep
// this module's imports to the profile contract and the session keys:
// anything else it pulls lands on the first paint.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProfile, type ProfilePatch } from "@pidash/api-client";

import { getAppClient } from "../../core/api/client.js";
import { sessionKeys } from "../../core/session/queries.js";

/**
 * Profile writes: role, use cases, marketing consent, progress flags
 * (AUTH-031–033, AUTH-039). Refreshes the cached profile.
 */
export function useUpdateProfile() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ProfilePatch) => updateProfile(getAppClient(), input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.profile });
    },
  });
}
