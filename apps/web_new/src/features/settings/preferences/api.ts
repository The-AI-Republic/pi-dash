// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Preference writes (parity/specs/profile.md §7, PROF-016/017). Each row
// saves the moment it is picked and reports through the shared toaster.
// Week start and the timezone reuse the shell's shared mutations so one
// code path owns those endpoints; the theme and language picks go through
// their engines, which own the immediate re-skin and re-render.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProfile, type Profile } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { sessionKeys } from "../../../core/session/index.js";

export const WEEK_START_SAVED_TITLE = "First day of the week updated successfully.";
export const WEEK_START_FAILED_TITLE = "Couldn't update the first day of the week";

/**
 * Persist the first day of the week through the profile endpoint. The
 * profile cache takes the server's answer at once, so every calendar
 * reading the stored index re-renders without a refetch.
 */
export function useSaveWeekStart() {
  const queryClient = useQueryClient();
  return useMutation<Profile, Error, number>({
    mutationFn: (startOfWeek) => updateProfile(getAppClient(), { start_of_the_week: startOfWeek }),
    onSuccess: (profile) => {
      queryClient.setQueryData(sessionKeys.profile, profile);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: sessionKeys.profile });
    },
  });
}
