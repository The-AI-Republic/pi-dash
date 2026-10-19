// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor mention lookup (parity/specs/issues.md §11, ISS-127): the
// description editor's person search. One GET returns a section per
// requested entity kind; the modal only asks for people, so the
// contract keeps that section and ignores the rest of the envelope.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/** One person suggestion: the member id and the name to show. */
export const MentionPerson = z.object({
  member__id: z.string(),
  member__display_name: z.string(),
  member__avatar_url: z.nullable(z.string()),
});
export type MentionPerson = z.infer<typeof MentionPerson>;

/** The envelope holds one array per entity kind; only people are read. */
export const MentionSearchResponse = z.object({
  user_mention: z.optional(z.array(MentionPerson)),
});
export type MentionSearchResponse = z.infer<typeof MentionSearchResponse>;

export interface MentionSearchQuery {
  /** Term typed after the @ trigger. */
  query: string;
  /** Project the editor is scoped to (limits people to its members). */
  projectId?: string | undefined;
  /** How many suggestions the server may return per kind. */
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
