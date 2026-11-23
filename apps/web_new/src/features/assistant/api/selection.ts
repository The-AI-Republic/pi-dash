// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface model choice reads and writes (NEWPIDASH-394, design
// sections 6.1 and 7.5). The composer picker reads the saved and
// effective choice for its own surface and writes back through PUT
// model-selection/{surface}; Settings (part 3) reuses the same reads
// for all three surfaces. The query key stays workspace-scoped like the
// other assistant reads, even though the answer is user-level.
import { queryOptions, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  assistantErrorFrom,
  deleteModelSelection,
  getModelSelection,
  putModelSelection,
  type ApiClient,
  type AssistantSurface,
  type ModelSelection,
  type ModelSelectionPutInput,
  type ModelSelectionSurfaceState,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { assistantKeys } from "./keys.js";

/** Shown when a selection write fails without an assistant-shaped refusal. */
export const SELECTION_FAILED_FALLBACK = "Something went wrong. Please try again.";

/**
 * The inline detail for a failed selection write: the assistant detail
 * when the server sent one, then its machine code, then a sentence.
 */
export function selectionFailureDetail(error: unknown): string {
  const failure = assistantErrorFrom(error);
  if (failure !== null) {
    if (failure.detail.trim() !== "") return failure.detail;
    if (failure.code.trim() !== "") return failure.code;
  }
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  return SELECTION_FAILED_FALLBACK;
}

/**
 * The composer's own surface (design section 6.1): the desktop build
 * writes `desktop_assistant`, the browser writes `web_assistant`. Which
 * surface a turn ran on is stamped server-side; this only names the
 * endpoint slot the picker writes.
 */
export function composerSurface(): AssistantSurface {
  return platform.agentRuntime === undefined ? "web_assistant" : "desktop_assistant";
}

/** The saved and effective choice for every surface. */
export function modelSelectionQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: assistantKeys.selection(workspaceSlug),
    queryFn: (): Promise<ModelSelection> => getModelSelection(client),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useModelSelection(workspaceSlug: string) {
  return useQuery(modelSelectionQueryOptions(getAppClient(), workspaceSlug));
}

export interface ModelSelectionWrites {
  save(surface: AssistantSurface, input: ModelSelectionPutInput): Promise<ModelSelectionSurfaceState>;
  clear(surface: AssistantSurface): Promise<void>;
}

/** The client-bound writes, so tests can drive them without React. */
export function modelSelectionActions(client: ApiClient): ModelSelectionWrites {
  return {
    save: (surface, input) => putModelSelection(client, surface, input),
    clear: (surface) => deleteModelSelection(client, surface),
  };
}

/**
 * One write, one refresh. The selection read is the single source of
 * the picker's value and notice, so every write invalidates it; the
 * connections list and the composer's config read refresh alongside so
 * Settings and the composer agree on the new value.
 */
export function useModelSelectionActions(): ModelSelectionWrites & {
  invalidate(workspaceSlug: string): Promise<void>;
} {
  const queryClient = useQueryClient();
  const client = getAppClient();
  const actions = modelSelectionActions(client);
  return {
    ...actions,
    invalidate: (workspaceSlug: string): Promise<void> =>
      Promise.all([
        queryClient.invalidateQueries({ queryKey: assistantKeys.selection(workspaceSlug) }),
        queryClient.invalidateQueries({ queryKey: assistantKeys.config(workspaceSlug) }),
      ]).then(() => undefined),
  };
}
