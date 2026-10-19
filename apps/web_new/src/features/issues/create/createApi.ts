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
  attachDescriptionAssets,
  createDraftIssue,
  generateAssistantTitle,
  getIssue,
  promoteDraftIssue,
  removeIssueFromCycle,
  runProjectEditorTask,
  searchIssues,
  searchMentionPeople,
  setIssueModules,
  updateDraftIssue,
  updateIssue,
  uploadDescriptionAsset,
  type ApiClient,
  type DescriptionAssetScope,
  type DraftIssue,
  type DraftIssueCreateInput,
  type Issue,
  type IssuePatch,
  type IssueSearchHit,
  type IssueSearchResults,
  type PromotedIssue,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";
import { TITLE_GENERATION_TIMEOUT_MS, type CreateMembership } from "./createForm.js";
import { changedCycleId, changedModuleIds, type EditMembership } from "./editForm.js";

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

/** Query key for the detail read an issue edit opens with (ISS-126). */
export function editDetailKey(workspaceSlug: string, projectId: string, issueId: string) {
  return [...workspaceScope(workspaceSlug), "issues", "create", "edit-detail", projectId, issueId] as const;
}

/**
 * The stored issue an edit opens, read so the form carries the real
 * description: list snapshots do not. A failed read leaves the snapshot
 * in place rather than blocking the edit.
 */
export function useEditIssueDetail(
  workspaceSlug: string,
  projectId: string | null,
  issueId: string | null,
  enabled: boolean
) {
  return useQuery<Issue>({
    queryKey: editDetailKey(workspaceSlug, projectId ?? "", issueId ?? ""),
    queryFn: () => getIssue(getAppClient(), workspaceSlug, projectId ?? "", issueId ?? ""),
    enabled: enabled && projectId !== null && issueId !== null,
    staleTime: REFERENCE_STALE_TIME_MS,
    retry: false,
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

/** The address of the row an edit targets: a live issue or a draft. */
export type EditTarget = { kind: "issue"; projectId: string; issueId: string } | { kind: "draft"; draftId: string };

/** Everything one edit write needs beyond its PATCH (ISS-126). */
export interface EditWriteInput {
  target: EditTarget;
  patch: IssuePatch;
  membership: EditMembership;
  /** The project a draft now stores; null clears it (ISS-126/130). */
  draftProject?: string | null | undefined;
}

/**
 * Sequence an edit's bridge writes after its PATCH (ISS-124/ISS-126):
 * the old cycle bridge is detached first, the new one attached second,
 * and modules follow, each awaited. A failure stops the chain.
 */
export async function applyEditMembership(
  client: ApiClient,
  workspaceSlug: string,
  target: EditTarget,
  membership: EditMembership
): Promise<void> {
  // Drafts carry their memberships on the draft itself (see
  // writeIssueEdit); no bridge call exists for them.
  if (target.kind === "draft") return;
  const { projectId, issueId } = target;
  if (membership.removedCycleId !== null) {
    await removeIssueFromCycle(client, workspaceSlug, projectId, membership.removedCycleId, issueId);
  }
  if (membership.addedCycleId !== null) {
    await addIssueToCycle(client, workspaceSlug, projectId, membership.addedCycleId, issueId);
  }
  if (membership.addedModuleIds.length > 0 || membership.removedModuleIds.length > 0) {
    await setIssueModules(client, workspaceSlug, projectId, issueId, {
      modules: membership.addedModuleIds,
      ...(membership.removedModuleIds.length === 0 ? {} : { removed_modules: membership.removedModuleIds }),
    });
  }
}

/** Patch a live issue or a draft, then reconcile its memberships. */
export async function writeIssueEdit(client: ApiClient, workspaceSlug: string, input: EditWriteInput): Promise<void> {
  const { target } = input;
  if (target.kind === "draft") {
    // A draft stores cycle, module and estimate picks on itself; the
    // update serializer replaces both sets, so the complete values ride
    // along with the changed fields (ISS-124/ISS-126).
    const draft = draftInputFromPatch(input.patch, input.membership, input.draftProject);
    if (Object.keys(draft).length > 0) {
      await updateDraftIssue(client, workspaceSlug, target.draftId, draft);
    }
    return;
  }
  if (Object.keys(input.patch).length > 0) {
    await updateIssue(client, workspaceSlug, target.projectId, target.issueId, input.patch);
  }
  await applyEditMembership(client, workspaceSlug, target, input.membership);
}

export function useWriteIssueEdit(workspaceSlug: string) {
  return useMutation<void, Error, EditWriteInput>({
    mutationFn: (input) => writeIssueEdit(getAppClient(), workspaceSlug, input),
  });
}

/**
 * The draft contract's slice of an issue patch: drafts hold the fields
 * the detail form shows, not the bridges, branch or execution target.
 */
export function draftInputFromPatch(
  patch: IssuePatch,
  membership?: EditMembership | undefined,
  project?: string | null | undefined
): DraftIssueCreateInput {
  const draft: DraftIssueCreateInput = {};
  if (project !== undefined) draft.project_id = project;
  if (patch.name !== undefined) draft.name = patch.name;
  if (patch.description_html !== undefined) draft.description_html = patch.description_html;
  if (patch.state_id !== undefined) draft.state_id = patch.state_id;
  if (patch.priority !== undefined) draft.priority = patch.priority;
  if (patch.parent_id !== undefined) draft.parent_id = patch.parent_id;
  if (patch.assignee_ids !== undefined) draft.assignee_ids = patch.assignee_ids;
  if (patch.label_ids !== undefined) draft.label_ids = patch.label_ids;
  if (patch.start_date !== undefined) draft.start_date = patch.start_date;
  if (patch.target_date !== undefined) draft.target_date = patch.target_date;
  if (patch.estimate_point !== undefined) draft.estimate_point = patch.estimate_point;
  if (membership !== undefined) {
    const cycleId = changedCycleId(membership);
    if (cycleId !== undefined) draft.cycle_id = cycleId;
    const moduleIds = changedModuleIds(membership);
    if (moduleIds !== undefined) draft.module_ids = moduleIds;
  }
  return draft;
}

/**
 * Publish a draft (ISS-130). The promote endpoint files the issue under
 * the project stored on the draft, not the one in its body, so a draft
 * that only just picked a project is patched first. Without that write
 * the server refuses a project-less draft even though the form names one.
 */
export async function publishDraft(
  client: ApiClient,
  workspaceSlug: string,
  draftId: string,
  input: DraftIssueCreateInput,
  storedProjectId: string | null
): Promise<PromotedIssue> {
  const projectId = input.project_id ?? null;
  if (projectId !== null && projectId !== storedProjectId) {
    await updateDraftIssue(client, workspaceSlug, draftId, { project_id: projectId });
  }
  return promoteDraftIssue(client, workspaceSlug, draftId, input);
}

export function usePublishDraft(workspaceSlug: string) {
  return useMutation<
    PromotedIssue,
    Error,
    { draftId: string; input: DraftIssueCreateInput; storedProjectId: string | null }
  >({
    mutationFn: ({ draftId, input, storedProjectId }) =>
      publishDraft(getAppClient(), workspaceSlug, draftId, input, storedProjectId),
  });
}

/** Description editor task that drafts a body from the title (ISS-127). */
export const DESCRIBE_TASK = "Generate a proper description for this work item.";
/** Toast when the title is too thin to describe. */
export const DESCRIBE_EMPTY_TITLE = "Couldn't write a description";
export const DESCRIBE_EMPTY_TITLE_BODY = "The title needs a little more detail before the assistant can describe it.";
/** Toast when the assistant refuses, answers nothing or times out. */
export const DESCRIBE_FAILED_TITLE = "Couldn't write a description";
export const DESCRIBE_FAILED_BODY = "Try again in a moment, or write the description yourself.";
/** Maximum assistant description requests per member per month. */
export const DESCRIBE_MONTHLY_LIMIT = 50;
export const DESCRIBE_RATE_LIMITED_BODY = `${DESCRIBE_MONTHLY_LIMIT} descriptions per month is the limit on this plan.`;

/** Generate a description body from the item's title. */
export async function requestDescription(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  title: string
): Promise<string> {
  const answer = await withTimeout(
    runProjectEditorTask(client, workspaceSlug, projectId, { task: DESCRIBE_TASK, prompt: title }),
    TITLE_GENERATION_TIMEOUT_MS
  );
  return answer.response_html.trim();
}

/** Run an assistant writing prompt and keep the markup it answers. */
export async function requestEditorAnswer(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  prompt: string
): Promise<string> {
  const answer = await withTimeout(
    runProjectEditorTask(client, workspaceSlug, projectId, { task: prompt }),
    TITLE_GENERATION_TIMEOUT_MS
  );
  return answer.response_html.trim();
}

export function useDescribeFromTitle(workspaceSlug: string) {
  return useMutation<string, Error, { projectId: string; title: string }>({
    mutationFn: ({ projectId, title }) => requestDescription(getAppClient(), workspaceSlug, projectId, title),
  });
}

export function useEditorAnswer(workspaceSlug: string) {
  return useMutation<string, Error, { projectId: string; prompt: string }>({
    mutationFn: ({ projectId, prompt }) => requestEditorAnswer(getAppClient(), workspaceSlug, projectId, prompt),
  });
}

/** How many people one mention search may offer. */
export const MENTION_RESULT_LIMIT = 5;

/** One mention suggestion row the editor's @ popup renders. */
export interface MentionSearchResults {
  people: { id: string; name: string }[];
}

/** Mention lookup for the description editor (ISS-127). */
export function mentionSearchKey(workspaceSlug: string, projectId: string, text: string) {
  return [...workspaceScope(workspaceSlug), "issues", "create", "mention-search", projectId, text] as const;
}

/** People matching the text typed after an @; none without a project. */
export async function requestMentions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  query: string
): Promise<MentionSearchResults> {
  if (projectId === "") return { people: [] };
  const people = await searchMentionPeople(client, workspaceSlug, {
    query,
    projectId,
    count: MENTION_RESULT_LIMIT,
  });
  return { people: people.map((person) => ({ id: person.member__id, name: person.member__display_name })) };
}

export function useMentionSearch(workspaceSlug: string, projectId: string, query: string, enabled: boolean) {
  return useQuery({
    queryKey: mentionSearchKey(workspaceSlug, projectId, query),
    queryFn: () => requestMentions(getAppClient(), workspaceSlug, projectId, query),
    staleTime: REFERENCE_STALE_TIME_MS,
    enabled: enabled && projectId !== "",
  });
}

/** One description image upload: the parked asset and its shown src. */
export interface DescriptionUploadInput {
  projectId: string;
  scope: DescriptionAssetScope;
  file: File;
}

export function useUploadDescriptionAsset(workspaceSlug: string) {
  return useMutation({
    mutationFn: (input: DescriptionUploadInput) =>
      uploadDescriptionAsset(getAppClient(), workspaceSlug, input.projectId, input.scope, input.file),
  });
}

/** Link parked description images to the row they were uploaded for. */
export async function applyDescriptionAssets(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  assetIds: readonly string[]
): Promise<void> {
  await attachDescriptionAssets(client, workspaceSlug, projectId, issueId, assetIds);
}

export function useAttachDescriptionAssets(workspaceSlug: string) {
  return useMutation<void, Error, { projectId: string; issueId: string; assetIds: string[] }>({
    mutationFn: ({ projectId, issueId, assetIds }) =>
      applyDescriptionAssets(getAppClient(), workspaceSlug, projectId, issueId, assetIds),
  });
}
