// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal-access-token reads and writes (parity/specs/workspace-settings.md
// §§25-28, WSET-025-028). Tokens belong to the signed-in user, so the key is
// user-level rather than workspace-scoped; sign-out clears the shared cache.
// Creates append the answered row (the secret stays in the create result),
// deletes remove optimistically and roll back on failure, and every failure
// reports through the shell's notices.

import { queryOptions, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createPersonalApiToken,
  deletePersonalApiToken,
  isApiError,
  listPersonalApiTokens,
  type ApiClient,
  type CreatedPersonalApiToken,
  type CreatePersonalApiTokenInput,
  type PersonalApiToken,
  type PersonalApiTokenList,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { applyOptimisticUpdate, type Rollback } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { TOKEN_CREATE_FAILED_TITLE, TOKEN_DELETE_FAILED_TITLE, TOKEN_DELETED_NOTICE } from "./model.js";

export const personalTokenKeys = {
  all: ["personal-api-tokens"] as const,
  list: ["personal-api-tokens", "list"] as const,
};

/** The caller's own non-service tokens, newest first. */
export function personalTokensQueryOptions(client: ApiClient) {
  return queryOptions({
    queryKey: personalTokenKeys.list,
    queryFn: () => listPersonalApiTokens(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function usePersonalTokens() {
  return useQuery(personalTokensQueryOptions(getAppClient()));
}

/** Append the server's answered row unless an identical row is already cached. */
export function appendPersonalToken(
  previous: PersonalApiTokenList | undefined,
  created: PersonalApiToken
): PersonalApiTokenList {
  if (previous === undefined) return [created];
  if (previous.some((row) => row.id === created.id)) return previous;
  return [created, ...previous];
}

/** Remove one row from a cached list. */
export function removePersonalToken(
  previous: PersonalApiTokenList | undefined,
  tokenId: string
): PersonalApiTokenList | undefined {
  return previous?.filter((row) => row.id !== tokenId);
}

/** Server detail for a failed write, when the error carries one. */
export function tokenFailureDetail(error: unknown): string | undefined {
  if (isApiError(error) && error.message !== "") return error.message;
  return error instanceof Error && error.message !== "" ? error.message : undefined;
}

/** Create a token and show its answered row under the list. */
export function useCreatePersonalToken() {
  const queryClient = useQueryClient();
  return useMutation<CreatedPersonalApiToken, Error, CreatePersonalApiTokenInput>({
    mutationFn: (input) => createPersonalApiToken(getAppClient(), input),
    onSuccess: (created) => {
      queryClient.setQueryData(personalTokenKeys.list, (previous: PersonalApiTokenList | undefined) =>
        appendPersonalToken(previous, created)
      );
    },
    onError: (error) => {
      notifyError(TOKEN_CREATE_FAILED_TITLE, tokenFailureDetail(error));
    },
  });
}

/** Delete a token with an optimistic removal, restoring the row on failure. */
export function useDeletePersonalToken() {
  const queryClient = useQueryClient();
  return useMutation<number, Error, string, Rollback>({
    mutationFn: (tokenId) => deletePersonalApiToken(getAppClient(), tokenId),
    onMutate: (tokenId) => {
      const rollback = applyOptimisticUpdate<PersonalApiTokenList>(queryClient, personalTokenKeys.list, (previous) =>
        removePersonalToken(previous, tokenId)
      );
      return rollback;
    },
    onSuccess: () => {
      notifySuccess(TOKEN_DELETED_NOTICE);
    },
    onError: (error, _tokenId, rollback) => {
      rollback?.();
      notifyError(TOKEN_DELETE_FAILED_TITLE, tokenFailureDetail(error));
    },
  });
}
