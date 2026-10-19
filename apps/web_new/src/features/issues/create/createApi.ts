// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Network helpers for the create dialog (parity/specs/issues.md §11):
// the assistant title generation, the Save-to-drafts write behind the
// discard guard (ISS-120) and the debounced parent search shared by the
// parent picker (ISS-122). Each one keeps the dialog component free of
// direct client calls.

import { useMutation, useQuery } from "@tanstack/react-query";
import {
  addIssueToCycle,
  createDraftIssue,
  generateAssistantTitle,
  searchIssues,
  setIssueModules,
  type ApiClient,
  type DraftIssue,
  type DraftIssueCreateInput,
  type IssueSearchHit,
  type IssueSearchResults,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { TITLE_GENERATION_TIMEOUT_MS, type CreateMembership } from "./createForm.js";

/** Toast copy after an inline draft save. */
export const DRAFT_SAVED_INLINE_TITLE = "Draft saved";
/** Toast copy when an inline draft save fails. */
export const DRAFT_SAVE_FAILED_INLINE_TITLE = "Couldn't save the draft";
/** Toast when a created item could not be attached to its cycle or modules. */
export const MEMBERSHIP_FAILED_TITLE = "Work item created, but not placed";
export const MEMBERSHIP_FAILED_BODY = "Add it to its cycle or modules from the item when you're ready.";
/** Toast copy when the assistant cannot name the work item. */
export const TITLE_GENERATION_FAILED_TITLE = "Couldn't generate a title";
export const TITLE_EMPTY_DESCRIPTION_BODY = "Add a description first so the assistant has something to name it from.";
export const TITLE_GENERATION_FAILED_BODY = "Enter a title yourself, or try again in a moment.";
export const TITLE_GENERATION_TIMED_OUT_BODY = "The assistant took too long. Enter a title yourself or try again.";
export const TITLE_GENERATION_EMPTY_BODY = "The assistant returned no title. Enter one yourself or try again.";

/** Parent-search debounce, from the row (ISS-122). */
export const PARENT_SEARCH_DEBOUNCE_MS = 500;

/**
 * Throw when the title helper does not answer in time. The helper has no
 * cancellation hook, so the late answer is simply ignored.
 */
export function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = globalThis.setTimeout(() => reject(new TitleTimeoutError()), timeoutMs);
    promise.then(
      (value) => {
        globalThis.clearTimeout(timer);
        resolve(value);
      },
      (error: unknown) => {
        globalThis.clearTimeout(timer);
        reject(error);
      }
    );
  });
}

export class TitleTimeoutError extends Error {
  constructor() {
    super("title-generation-timeout");
    this.name = "TitleTimeoutError";
  }
}

/** Ask the assistant to name a work item from its description (ISS-118). */
export async function requestGeneratedTitle(
  client: ApiClient,
  workspaceSlug: string,
  description: string
): Promise<string> {
  const answer = await withTimeout(
    generateAssistantTitle(client, workspaceSlug, description),
    TITLE_GENERATION_TIMEOUT_MS
  );
  return answer.title.trim();
}

/** Run the assistant title helper through the query cache of the dialog. */
export function useGenerateTitle(workspaceSlug: string) {
  return useMutation<string, Error, string>({
    mutationFn: (description: string) => requestGeneratedTitle(getAppClient(), workspaceSlug, description),
  });
}

/**
 * Save the discard guard's form as a workspace draft (ISS-120). The
 * discard prompt shows its own "Draft saved" notice once the write lands,
 * so the mutation stays silent; the form's inline save announces itself.
 */
export function useSaveDraft(workspaceSlug: string) {
  return useMutation<DraftIssue, Error, DraftIssueCreateInput>({
    mutationFn: (input) => createDraftIssue(getAppClient(), workspaceSlug, input),
  });
}

/** Query key prefix for the parent picker's project searches. */
export function parentSearchKey(workspaceSlug: string, projectId: string, text: string) {
  return [...workspaceScope(workspaceSlug), "issues", "create", "parent-search", projectId, text] as const;
}

/** One project search for parent candidates; an empty term lists the project's issues. */
export function parentSearchQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, text: string) {
  return {
    queryKey: parentSearchKey(workspaceSlug, projectId, text),
    queryFn: (): Promise<IssueSearchResults> =>
      searchIssues(client, workspaceSlug, projectId, { search: text, parent: true }),
    staleTime: REFERENCE_STALE_TIME_MS,
    enabled: projectId !== "",
  };
}

/** Parent candidates for a project; the caller debounces the term. */
export function useParentSearch(workspaceSlug: string, projectId: string, text: string, enabled: boolean) {
  return useQuery({
    ...parentSearchQueryOptions(getAppClient(), workspaceSlug, projectId, text),
    enabled: enabled && projectId !== "",
  });
}

/** One created item plus the membership the dialog must attach. */
export interface CreateMembershipInput extends CreateMembership {
  projectId: string;
  issueId: string;
}

/** Attach a newly created row to its cycle, then its modules (ISS-117).
 * The bridge writes run one after the other so they cannot race. */
export async function applyCreateMembership(
  client: ApiClient,
  workspaceSlug: string,
  input: CreateMembershipInput
): Promise<void> {
  if (input.cycleId !== null) {
    await addIssueToCycle(client, workspaceSlug, input.projectId, input.cycleId, input.issueId);
  }
  if (input.moduleIds.length > 0) {
    await setIssueModules(client, workspaceSlug, input.projectId, input.issueId, { modules: input.moduleIds });
  }
}

export function useApplyCreateMembership(workspaceSlug: string) {
  return useMutation<void, Error, CreateMembershipInput>({
    mutationFn: (input) => applyCreateMembership(getAppClient(), workspaceSlug, input),
  });
}

/** Identifier text for one search hit: short code plus sequence. */
export function hitIdentifier(hit: IssueSearchHit): string {
  return `${hit.project__identifier}-${hit.sequence_id}`;
}

/** Recent (or matching) candidates: the picker never repeats the current parent. */
export function selectParentCandidates(hits: readonly IssueSearchHit[], currentId: string | null): IssueSearchHit[] {
  return hits.filter((hit) => hit.id !== currentId);
}
