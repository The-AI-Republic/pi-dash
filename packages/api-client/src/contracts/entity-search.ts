// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor mention lookup (parity/specs/issues.md §11, ISS-127; comments
// CMT-019): the person search behind the @ trigger. One GET returns a
// section per requested entity kind; the picker reads the people section
// and renders the rest under their own headings, so the contract keeps
// every section the server sends instead of stripping unknown ones.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/** One person suggestion: the member id and the name to show. */
export const MentionPerson = z.object({
  member__id: z.string(),
  member__display_name: z.string(),
  member__avatar_url: z.nullable(z.string()),
});
export type MentionPerson = z.infer<typeof MentionPerson>;

/**
 * Per-kind suggestion cap the server applies when `count` is omitted.
 * The server slices each section to this many rows; it enforces no
 * maximum, so larger counts simply return more rows.
 */
export const MENTION_SEARCH_DEFAULT_COUNT = 5;

/**
 * The envelope holds one array per requested entity kind. The people
 * section is typed; every other section passes through untouched so a
 * picker can head and list future kinds without a contract change.
 */
export const MentionSearchResponse = z.looseObject({
  user_mention: z.optional(z.array(MentionPerson)),
});
export type MentionSearchResponse = z.infer<typeof MentionSearchResponse>;

export interface MentionSearchQuery {
  /** Term typed after the @ trigger. */
  query: string;
  /** Project the editor is scoped to (limits people to its members). */
  projectId?: string | undefined;
  /**
   * How many suggestions the server may return per kind. Defaults to
   * {@link MENTION_SEARCH_DEFAULT_COUNT} server-side; no maximum.
   */
  count?: number | undefined;
}

const USER_MENTION_ENTITY = "user_mention";

/** Search the people a mention may point at, in the editor's project. */
export async function searchMentionPeople(
  client: ApiClient,
  slug: string,
  query: MentionSearchQuery
): Promise<MentionPerson[]> {
  const path = `/api/workspaces/${slug}/entity-search/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      query: query.query,
      query_type: USER_MENTION_ENTITY,
      project_id: query.projectId,
      count: query.count,
    },
  });
  const parsed = client.parse(MentionSearchResponse, body, `GET ${path}`);
  return parsed.user_mention ?? [];
}

export interface EntitySearchQuery {
  /** Term typed after the trigger. */
  query: string;
  /**
   * Entity kinds to search, one section each (`user_mention`, `project`,
   * `issue`, `cycle`, `module`, `page`; unknown kinds are ignored
   * server-side). Defaults to the people section only.
   */
  queryTypes?: string[] | undefined;
  /** Project the search is scoped to (people limited to its members). */
  projectId?: string | undefined;
  /**
   * How many suggestions the server may return per kind. Defaults to
   * {@link MENTION_SEARCH_DEFAULT_COUNT} server-side; no maximum.
   */
  count?: number | undefined;
}

/**
 * Search every requested entity kind and keep the whole envelope: the
 * typed people section plus each other section untouched, so the picker
 * can head and list them. Unknown sections arrive untyped by design —
 * each owning area types its own kind.
 */
export async function searchEntitySections(
  client: ApiClient,
  slug: string,
  query: EntitySearchQuery
): Promise<MentionSearchResponse> {
  const path = `/api/workspaces/${slug}/entity-search/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      query: query.query,
      query_type: (query.queryTypes ?? [USER_MENTION_ENTITY]).join(","),
      project_id: query.projectId,
      count: query.count,
    },
  });
  return client.parse(MentionSearchResponse, body, `GET ${path}`);
}
