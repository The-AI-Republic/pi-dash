// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner row actions (NEWFRONT-540, RUN-010). Both writes re-read the list
// on success so the table and the rail agree; the caller keeps its
// confirmation open until the write settles.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { deleteRunner, revokeRunner, updateRunner, type RunnerPatch } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { runnersKeys } from "./keys.js";

/** Delete one runner through its confirmation. */
export function useDeleteRunner(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (runnerId: string) => deleteRunner(getAppClient(), runnerId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}

/** Revoke one runner's credentials, keeping its row. */
export function useRevokeRunner(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (runnerId: string) => revokeRunner(getAppClient(), runnerId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}

/**
 * Change one runner's pod or pod membership (owner-only PATCH, built-in
 * engine design §7). Re-reads the area so tiles, counts and members agree;
 * the caller catches the refusal and explains it.
 */
export function useUpdateRunner(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { runnerId: string; patch: RunnerPatch }) =>
      updateRunner(getAppClient(), input.runnerId, input.patch),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: runnersKeys.all(workspaceSlug) }),
  });
}
