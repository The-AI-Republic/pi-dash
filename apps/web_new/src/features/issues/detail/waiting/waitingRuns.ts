// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Waiting-run actions for the issue detail (hybrid assignment, design
// §10.3): release one queued run to its pod's shared queue, or cancel
// it. Both re-read the issue caches on settle and toast on failure;
// the release leaves the issue's dedicated assignment untouched.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { cancelRun, releaseRunPin } from "@pidash/api-client";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError } from "../../../../shared/shell/index.js";
import { issueKeys } from "../../api/keys.js";

/** Failure title when a waiting run cannot be released. */
export const RELEASE_RUN_FAILED_TITLE = "Couldn't release the run";
const RELEASE_RUN_FAILED_BODY = "The run is still waiting — check your connection and try again.";

/** Failure title when a waiting run cannot be cancelled. */
export const CANCEL_RUN_FAILED_TITLE = "Couldn't cancel the run";
const CANCEL_RUN_FAILED_BODY = "The run is still waiting — check your connection and try again.";

/** Release one queued pinned run to its pod; the issue stays dedicated. */
export function useReleaseWaitingRun(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<unknown, Error, string>({
    mutationFn: (runId) => releaseRunPin(getAppClient(), runId),
    onError: () => {
      notifyError(RELEASE_RUN_FAILED_TITLE, RELEASE_RUN_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
  return {
    isReleasing: mutation.isPending,
    releaseWaitingRun: (runId: string) => mutation.mutate(runId),
  };
}

/** Cancel a waiting run. */
export function useCancelWaitingRun(workspaceSlug: string) {
  const queryClient = useQueryClient();
  const mutation = useMutation<unknown, Error, string>({
    mutationFn: (runId) => cancelRun(getAppClient(), runId),
    onError: () => {
      notifyError(CANCEL_RUN_FAILED_TITLE, CANCEL_RUN_FAILED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: issueKeys.all(workspaceSlug) });
    },
  });
  return {
    isCancelling: mutation.isPending,
    cancelWaitingRun: (runId: string) => mutation.mutate(runId),
  };
}
