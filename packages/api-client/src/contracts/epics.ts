// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Epics: the issues surface scoped to epic-typed rows (an epic is an
// issue whose type carries is_epic; there is no Epic model). The old
// frontend's issue service swapped the `issues` path segment for `epics`
// and kept every other segment, so the epics collection and detail read
// mirror their issues counterparts. Two segments do change on the epics
// side: the children collection is `epics/<id>/issues/` and external
// links live at `epics/<id>/links/`. Move is not mirrored — it stays the
// shared `work-items/<id>/move/` endpoint. The collection, entry and
// children routes ride the OSS stack API ungated (NEWFRONT-984) and are
// proven by live contract tests; the archive and links mirrors stay
// unserved until a caller needs them. The cloud gate lives in the web_new
// edition module, not here. The stub tests in contracts.test.ts prove
// the URLs and payload parsing.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import {
  GroupedIssueListResponse,
  Issue,
  IssueListItem,
  IssueListResponse,
  SubGroupedIssueListResponse,
  issueListQueryParams,
} from "./issues.js";
import type { IssueCreateInput, IssuePatch } from "./issue-actions.js";
import { IssueCreateResult } from "./issue-actions.js";
import { AddSubIssuesResponse, SubIssuesResponse } from "./issue-subissues.js";
import { IssueArchiveResult, MovedIssue } from "./issue-lifecycle.js";
import { IssueLink, IssueLinkInput, IssueLinkList } from "./issue-links.js";

/**
 * The epics base path. Every epics-segment endpoint below hangs off it;
 * the only exception is move, which the backend serves from work-items.
 */
export function epicsCollection(slug: string, projectId: string): string {
  return `/api/workspaces/${slug}/projects/${projectId}/epics/`;
}

/**
 * One epic row. The list projection is the issues list row; the cloud
 * serializer may also stamp the epic marker, but readers must treat its
 * absence as "plain work item" rather than guessing (the old client
 * filled it in locally after the detail read, so a raw server row can
 * arrive without it).
 */
export const EpicListItem = z.object({
  ...IssueListItem.shape,
  is_epic: z.optional(z.boolean()),
});
export type EpicListItem = z.infer<typeof EpicListItem>;

/** Cursor-paginated envelope for the epic list; same shape as issues. */
export const EpicListResponse = z.object({
  ...IssueListResponse.shape,
  results: z.array(EpicListItem),
});
export type EpicListResponse = z.infer<typeof EpicListResponse>;

/** Grouped epic list: one bucket per group value, as for issues. */
export const GroupedEpicListResponse = GroupedIssueListResponse;
export type GroupedEpicListResponse = z.infer<typeof GroupedEpicListResponse>;

/** Sub-grouped epic list: buckets within buckets, as for issues. */
export const SubGroupedEpicListResponse = SubGroupedIssueListResponse;
export type SubGroupedEpicListResponse = z.infer<typeof SubGroupedEpicListResponse>;

export interface EpicListQuery {
  perPage?: number;
  cursor?: string;
  orderBy?: string;
  groupBy?: string;
  subGroupBy?: string;
  /** Legacy filter params (state, priority, assignees, ...) passed through verbatim. */
  filters?: Record<string, string | string[]>;
}

export async function listEpics(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: EpicListQuery = {}
): Promise<EpicListResponse> {
  const path = epicsCollection(slug, projectId);
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(EpicListResponse, body, `GET ${path}`);
}

/** Epic list with `group_by`: one bucket per group value. */
export async function listGroupedEpics(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: EpicListQuery & { groupBy: string }
): Promise<GroupedEpicListResponse> {
  const path = epicsCollection(slug, projectId);
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(GroupedEpicListResponse, body, `GET grouped ${path}`);
}

/** Epic list with `group_by` + `sub_group_by`: buckets within buckets. */
export async function listSubGroupedEpics(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: EpicListQuery & { groupBy: string; subGroupBy: string }
): Promise<SubGroupedEpicListResponse> {
  const path = epicsCollection(slug, projectId);
  const body = await client.getJson<unknown>(path, { query: issueListQueryParams(query) });
  return client.parse(SubGroupedEpicListResponse, body, `GET sub-grouped ${path}`);
}

/**
 * Epic detail: the issues detail fields plus the optional epic marker.
 * Discovery reads often come from the global `work-items/<IDENT-seq>/`
 * catalog first, which teaches the client that an item is an epic, so a
 * detail fetched through this path should carry `is_epic: true`; the
 * schema keeps it optional so a server that omits it still parses.
 */
export const Epic = z.object({
  ...Issue.shape,
  is_epic: z.optional(z.boolean()),
});
export type Epic = z.infer<typeof Epic>;

export async function getEpic(client: ApiClient, slug: string, projectId: string, epicId: string): Promise<Epic> {
  const path = `${epicsCollection(slug, projectId)}${epicId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(Epic, body, `GET ${path}`);
}

/** Epic create takes the same payload the issues create modal builds. */
export type EpicCreateInput = IssueCreateInput;

/** The created epic row: the list projection plus the deletion marker. */
export const EpicCreateResult = IssueCreateResult;
export type EpicCreateResult = z.infer<typeof EpicCreateResult>;

export async function createEpic(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: EpicCreateInput
): Promise<EpicCreateResult> {
  const path = epicsCollection(slug, projectId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(EpicCreateResult, envelope.data, `POST ${path}`);
}

/**
 * Epic update: the same writable field set as the issues update, answered
 * with 204 and no body. Cycle and module membership are not part of this
 * patch — they ride their own bridge calls exactly as they do for issues.
 */
export type EpicPatch = IssuePatch;

export async function updateEpic(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  input: EpicPatch
): Promise<void> {
  await client.patch(`${epicsCollection(slug, projectId)}${epicId}/`, { json: input });
}

/** Epic delete answers 204; project admins and the epic's creator only. */
export async function deleteEpic(client: ApiClient, slug: string, projectId: string, epicId: string): Promise<void> {
  await client.delete(`${epicsCollection(slug, projectId)}${epicId}/`);
}

/**
 * Archive stamps the epic with today's date and answers the stamp. Only
 * completed or cancelled epics archive; anything else is a 400, so
 * callers gate the entry on the state group.
 */
export const EpicArchiveResult = IssueArchiveResult;
export type EpicArchiveResult = z.infer<typeof EpicArchiveResult>;

export async function archiveEpic(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string
): Promise<EpicArchiveResult> {
  const path = `${epicsCollection(slug, projectId)}${epicId}/archive/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(EpicArchiveResult, envelope.data, `POST ${path}`);
}

/** Restore clears the archive stamp (204). */
export async function restoreEpic(client: ApiClient, slug: string, projectId: string, epicId: string): Promise<void> {
  await client.delete(`${epicsCollection(slug, projectId)}${epicId}/archive/`);
}

/**
 * Move is not mirrored under `epics/`: the backend serves one move
 * endpoint for every work item, so epics move through the same call and
 * answer the same row shape. The target takes a project UUID or its
 * workspace-scoped identifier; cross-workspace moves are a 400.
 */
export const MovedEpic = MovedIssue;
export type MovedEpic = z.infer<typeof MovedEpic>;

export async function moveEpicToProject(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  targetProject: string
): Promise<MovedEpic> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/work-items/${epicId}/move/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { project: targetProject } });
  return client.parse(MovedEpic, envelope.data, `POST ${path}`);
}

/**
 * The epics children collection is spelled `issues/`, not `sub-issues/`
 * — the epics segment already answers "which epics", so the second
 * segment names the children. The payload and page shape are the same
 * as the issues sub-issues collection.
 */
export const EpicWorkItemsResponse = SubIssuesResponse;
export type EpicWorkItemsResponse = z.infer<typeof EpicWorkItemsResponse>;

export const AddedEpicWorkItemsResponse = AddSubIssuesResponse;
export type AddedEpicWorkItemsResponse = z.infer<typeof AddedEpicWorkItemsResponse>;

function epicWorkItemsPath(slug: string, projectId: string, epicId: string): string {
  return `${epicsCollection(slug, projectId)}${epicId}/issues/`;
}

export async function listEpicWorkItems(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  query: { perPage?: number; cursor?: string; orderBy?: string } = {}
): Promise<EpicWorkItemsResponse> {
  const path = epicWorkItemsPath(slug, projectId, epicId);
  const body = await client.getJson<unknown>(path, {
    query: { per_page: query.perPage, cursor: query.cursor, order_by: query.orderBy },
  });
  return client.parse(EpicWorkItemsResponse, body, `GET ${path}`);
}

/**
 * Attach existing work items to the epic. An empty id list is a 400;
 * ids that are already attached or unknown are skipped.
 */
export async function addEpicWorkItems(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  workItemIds: string[]
): Promise<AddedEpicWorkItemsResponse> {
  const path = epicWorkItemsPath(slug, projectId, epicId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: { sub_issue_ids: workItemIds } });
  return client.parse(AddedEpicWorkItemsResponse, envelope.data, `POST ${path}`);
}

/**
 * External links on an epic live at `epics/<id>/links/` — the issues
 * collection is `issue-links/`, the epics one drops the prefix. Rows,
 * writes and server-side URL normalization are the same.
 */
export const EpicLink = IssueLink;
export type EpicLink = z.infer<typeof EpicLink>;
export const EpicLinkList = IssueLinkList;
export type EpicLinkList = z.infer<typeof EpicLinkList>;
export type EpicLinkInput = IssueLinkInput;

function epicLinksPath(slug: string, projectId: string, epicId: string): string {
  return `${epicsCollection(slug, projectId)}${epicId}/links/`;
}

export async function listEpicLinks(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string
): Promise<EpicLinkList> {
  const path = epicLinksPath(slug, projectId, epicId);
  const body = await client.getJson<unknown>(path);
  return client.parse(EpicLinkList, body, `GET ${path}`);
}

export async function createEpicLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  input: EpicLinkInput
): Promise<EpicLink> {
  const path = epicLinksPath(slug, projectId, epicId);
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(EpicLink, envelope.data, `POST ${path}`);
}

export async function updateEpicLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  linkId: string,
  input: Partial<EpicLinkInput>
): Promise<EpicLink> {
  const path = `${epicLinksPath(slug, projectId, epicId)}${linkId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(EpicLink, envelope.data, `PATCH ${path}`);
}

export async function deleteEpicLink(
  client: ApiClient,
  slug: string,
  projectId: string,
  epicId: string,
  linkId: string
): Promise<void> {
  await client.delete(`${epicLinksPath(slug, projectId, epicId)}${linkId}/`);
}
