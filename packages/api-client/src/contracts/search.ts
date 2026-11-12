// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace search contract: the global search endpoint behind the command
// palette's server search. One GET fans out across entity filters and
// answers `{results: {<entity>: [...]}}` with only the requested entities.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

export const SEARCH_ENTITIES = [
  "workspace",
  "project",
  "issue",
  "cycle",
  "module",
  "issue_view",
  "page",
  "intake",
] as const;
export type SearchEntity = (typeof SEARCH_ENTITIES)[number];

const WorkspaceSearchRow = z.object({
  name: z.string(),
  id: uuid(),
  slug: z.string(),
});

const ProjectSearchRow = z.object({
  name: z.string(),
  id: uuid(),
  identifier: z.string(),
  workspace__slug: z.string(),
});

const IssueSearchRow = z.object({
  name: z.string(),
  id: uuid(),
  sequence_id: z.number(),
  project__identifier: z.string(),
  project_id: uuid(),
  workspace__slug: z.string(),
});

const ContainerSearchRow = z.object({
  name: z.string(),
  id: uuid(),
  project_id: uuid(),
  project__identifier: z.string(),
  workspace__slug: z.string(),
});

const PageSearchRow = z.object({
  name: z.string(),
  id: uuid(),
  project_ids: z.array(uuid()),
  project_identifiers: z.array(z.string()),
  workspace__slug: z.string(),
});

/**
 * Per-entity result lists. Every key is optional because the caller picks
 * entities with the `entities` param; unrequested filters are absent.
 */
export const SearchResults = z.object({
  results: z.object({
    workspace: z.optional(z.array(WorkspaceSearchRow)),
    project: z.optional(z.array(ProjectSearchRow)),
    issue: z.optional(z.array(IssueSearchRow)),
    cycle: z.optional(z.array(ContainerSearchRow)),
    module: z.optional(z.array(ContainerSearchRow)),
    issue_view: z.optional(z.array(ContainerSearchRow)),
    page: z.optional(z.array(PageSearchRow)),
    intake: z.optional(z.array(IssueSearchRow)),
  }),
});
export type SearchResults = z.infer<typeof SearchResults>;

export interface WorkspaceSearchQuery {
  /** Free text matched against names (and issue titles, bodies, comments). */
  search: string;
  /** Narrow project-scoped entities to one project. */
  projectId?: string;
  /**
   * True searches every project in the workspace; false (the default)
   * scopes project entities to `projectId` when one is given.
   */
  workspaceSearch?: boolean;
  /** Entities to run; defaults to all server-side when omitted. */
  entities?: SearchEntity[];
}

export async function searchWorkspace(
  client: ApiClient,
  slug: string,
  query: WorkspaceSearchQuery
): Promise<SearchResults> {
  const body = await client.getJson<unknown>(`/api/workspaces/${slug}/search/`, {
    query: {
      search: query.search,
      project_id: query.projectId,
      workspace_search: query.workspaceSearch,
      entities: query.entities?.join(","),
    },
  });
  return client.parse(SearchResults, body, `GET /api/workspaces/${slug}/search/`);
}
