// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Widget reads and writes (parity/specs/issues.md §15, ISS-179–186).
// Relations and links each own a query under the issue scope; the
// writes invalidate the detail row (the header counts) and the widget
// read so the server's reciprocal edge is what the next paint shows.

import { queryOptions, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  addIssueRelations,
  createIssueLink,
  deleteIssueLink,
  isApiError,
  listIssueLinks,
  listIssueRelations,
  removeIssueRelation,
  searchIssues,
  updateIssueLink,
  type ApiClient,
  type IssueLink,
  type IssueLinkInput,
  type IssueLinkList,
  type IssueRelations,
  type IssueSearchHit,
  type IssueSearchResults,
} from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { REFERENCE_STALE_TIME_MS } from "../../../core/query/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { relationRows } from "./relationsModel.js";
import type { RelationKind } from "./relationsModel.js";

/** Relations of one issue, grouped by direction. */
export function relationsQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: issueKeys.widgetRelations(workspaceSlug, issueId),
    queryFn: () => listIssueRelations(client, workspaceSlug, projectId, issueId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

/** External links of one issue, newest first. */
export function linksQueryOptions(client: ApiClient, workspaceSlug: string, projectId: string, issueId: string) {
  return queryOptions({
    queryKey: issueKeys.widgetLinks(workspaceSlug, issueId),
    queryFn: () => listIssueLinks(client, workspaceSlug, projectId, issueId),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useIssueRelations(workspaceSlug: string, projectId: string, issueId: string, enabled = true) {
  return useQuery({ ...relationsQueryOptions(getAppClient(), workspaceSlug, projectId, issueId), enabled });
}

export function useIssueLinks(workspaceSlug: string, projectId: string, issueId: string, enabled = true) {
  return useQuery({ ...linksQueryOptions(getAppClient(), workspaceSlug, projectId, issueId), enabled });
}

/** One page of candidate work items for the relation picker. */
export function relationCandidatesQueryOptions(
  client: ApiClient,
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  query: string,
  workspaceSearch: boolean
) {
  return queryOptions({
    queryKey: issueKeys.widgetCandidates(workspaceSlug, issueId, query, workspaceSearch),
    queryFn: () =>
      searchIssues(client, workspaceSlug, projectId, {
        issueId,
        issueRelation: true,
        ...(query === "" ? {} : { search: query }),
        ...(workspaceSearch ? { workspaceSearch: true } : {}),
      }),
    staleTime: REFERENCE_STALE_TIME_MS,
  });
}

export function useRelationCandidates(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  query: string,
  workspaceSearch: boolean,
  enabled: boolean
) {
  return useQuery({
    ...relationCandidatesQueryOptions(getAppClient(), workspaceSlug, projectId, issueId, query, workspaceSearch),
    enabled,
  });
}

function refreshIssueWidgets(queryClient: QueryClient, workspaceSlug: string, issueId: string) {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: issueKeys.widgetRelations(workspaceSlug, issueId) }),
    queryClient.invalidateQueries({ queryKey: issueKeys.widgetLinks(workspaceSlug, issueId) }),
    queryClient.invalidateQueries({ queryKey: issueKeys.detail(workspaceSlug, issueId) }),
    queryClient.invalidateQueries({ queryKey: [...issueKeys.all(workspaceSlug), "browse"] }),
    queryClient.invalidateQueries({ queryKey: [...issueKeys.all(workspaceSlug), "list"] }),
  ]).then(() => undefined);
}

export interface RelationWriteScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/** Add one or more issues under a kind; the server stores both sides. */
export function useAddRelations(scope: RelationWriteScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { kind: RelationKind; issueIds: string[] }) =>
      addIssueRelations(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, {
        relationType: input.kind,
        issues: input.issueIds,
      }),
    onSuccess: () => notifySuccess("Relations added"),
    onError: (error: Error) => notifyError("Couldn't add the relations", error.message),
    onSettled: () => refreshIssueWidgets(queryClient, scope.workspaceSlug, scope.issueId),
  });
}

/** Drop one row from the cached grouped read; undefined when it is absent. */
export function withoutRelationRow(
  relations: IssueRelations | undefined,
  kind: RelationKind,
  relatedIssueId: string
): IssueRelations | undefined {
  if (relations === undefined) return undefined;
  return { ...relations, [kind]: relations[kind].filter((row) => row.id !== relatedIssueId) };
}

/**
 * Detach one edge by kind; the server removes the reciprocal too. The
 * row disappears immediately (ISS-181) and comes back with the cached
 * read when the write fails.
 */
export function useRemoveRelation(scope: RelationWriteScope) {
  const queryClient = useQueryClient();
  const key = issueKeys.widgetRelations(scope.workspaceSlug, scope.issueId);
  return useMutation({
    mutationFn: (input: { kind: RelationKind; relatedIssue: string }) =>
      removeIssueRelation(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, {
        relationType: input.kind,
        relatedIssue: input.relatedIssue,
      }),
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<IssueRelations>(key);
      queryClient.setQueryData<IssueRelations | undefined>(key, (current) =>
        withoutRelationRow(current, input.kind, input.relatedIssue)
      );
      return { previous };
    },
    onSuccess: () => notifySuccess("Relation removed"),
    onError: (error: Error, _input, context) => {
      if (context?.previous !== undefined) queryClient.setQueryData(key, context.previous);
      notifyError("Couldn't remove the relation", error.message);
    },
    onSettled: () => refreshIssueWidgets(queryClient, scope.workspaceSlug, scope.issueId),
  });
}

/** Paste a missing scheme; anything already starting with http(s) is kept. */
export function normalizeLinkUrl(raw: string): string {
  const trimmed = raw.trim();
  return /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
}

export interface LinkWriteScope {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

export function useCreateLink(scope: LinkWriteScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: IssueLinkInput) =>
      createIssueLink(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, input),
    onSuccess: () => notifySuccess("Link added"),
    onError: (error: Error) => notifyError("Couldn't add the link", linkFailureDetail(error)),
    onSettled: () => refreshIssueWidgets(queryClient, scope.workspaceSlug, scope.issueId),
  });
}

export function useUpdateLink(scope: LinkWriteScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { linkId: string; patch: Partial<IssueLinkInput> }) =>
      updateIssueLink(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, input.linkId, input.patch),
    onSuccess: () => notifySuccess("Link updated"),
    onError: (error: Error) => notifyError("Couldn't update the link", linkFailureDetail(error)),
    onSettled: () => refreshIssueWidgets(queryClient, scope.workspaceSlug, scope.issueId),
  });
}

export function useDeleteLink(scope: LinkWriteScope) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (linkId: string) =>
      deleteIssueLink(getAppClient(), scope.workspaceSlug, scope.projectId, scope.issueId, linkId),
    onSuccess: () => notifySuccess("Link removed"),
    onError: (error: Error) => notifyError("Couldn't remove the link", error.message),
    onSettled: () => refreshIssueWidgets(queryClient, scope.workspaceSlug, scope.issueId),
  });
}

/** Server detail when it carries one, else the transport message. */
export function linkFailureDetail(error: Error): string {
  if (isApiError(error) && error.message !== "") return error.message;
  return error.message;
}

/** Rows from one kind in the relation read; empty before it loads. */
export function rowsOfKind(relations: IssueRelations | undefined, kind: RelationKind) {
  return relationRows(relations, kind);
}

/** Link rows newest first even if the server ever reorders. */
export function newestLinks(links: IssueLinkList | undefined): IssueLink[] {
  if (links === undefined) return [];
  return [...links].sort((a, b) => (a.created_at < b.created_at ? 1 : a.created_at > b.created_at ? -1 : 0));
}

/** Picker rows carry no epic flag; the browse route covers both kinds. */
export function candidateKey(row: IssueSearchHit): string {
  return `${row.project__identifier}-${row.sequence_id}`;
}

export type { IssueSearchHit, IssueSearchResults };
