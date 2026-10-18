// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-search contracts: the picker read behind the parent modal,
// the add-existing sub-issue modal and the add-relation modal. One
// endpoint with exclusion flags per caller; the answer is a flat
// capped array, never paginated.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

/** One search hit: identity plus the columns pickers render. */
export const IssueSearchHit = z.object({
  name: z.string(),
  id: uuid(),
  start_date: z.nullable(z.iso.date()),
  sequence_id: z.number(),
  project__name: z.string(),
  project__identifier: z.string(),
  project_id: uuid(),
  workspace__slug: z.string(),
  state__name: z.nullable(z.string()),
  state__group: z.nullable(z.string()),
  state__color: z.nullable(z.string()),
});
export type IssueSearchHit = z.infer<typeof IssueSearchHit>;

export const IssueSearchResults = z.array(IssueSearchHit);
export type IssueSearchResults = z.infer<typeof IssueSearchResults>;

export interface IssueSearchQuery {
  search?: string;
  /** Search the whole workspace instead of just this project. */
  workspaceSearch?: boolean;
  /** Parent picker: exclude the issue, its parent and its children. */
  parent?: boolean;
  /** Relation picker: exclude already-related issues. */
  issueRelation?: boolean;
  /** Add-existing picker: root issues only. */
  subIssue?: boolean;
  issueId?: string;
  /** Add-existing picker: exclude issues already attached to a cycle. */
  cycle?: boolean;
  /** Add-existing picker: exclude issues already in this module. */
  module?: string;
}

export async function searchIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  query: IssueSearchQuery = {}
): Promise<IssueSearchResults> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/search-issues/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      search: query.search,
      workspace_search: query.workspaceSearch === undefined ? undefined : String(query.workspaceSearch),
      parent: query.parent === undefined ? undefined : String(query.parent),
      issue_relation: query.issueRelation === undefined ? undefined : String(query.issueRelation),
      sub_issue: query.subIssue === undefined ? undefined : String(query.subIssue),
      issue_id: query.issueId,
      cycle: query.cycle === undefined ? undefined : String(query.cycle),
      module: query.module,
    },
  });
  return client.parse(IssueSearchResults, body, `GET ${path}`);
}
