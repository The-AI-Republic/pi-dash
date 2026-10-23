// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Editor lookup: the person search behind the @ trigger
// (parity/specs/issues.md §11, ISS-127; comments CMT-019) and the
// entity-preview search the page body uses for links and embeds
// (parity/specs/pages.md §11.5, PAGE-041/043). One GET returns a section per
// requested entity kind; the picker reads the people section and renders the
// rest under their own headings, so the envelope keeps every section the
// server sends instead of stripping unknown ones.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

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
/** One project suggestion from the entity preview. */
export const EntitySearchProject = z.object({
  name: z.string(),
  id: uuid(),
  identifier: z.string(),
  logo_props: z.record(z.string(), z.unknown()),
  workspace__slug: z.string(),
});
export type EntitySearchProject = z.infer<typeof EntitySearchProject>;

/** One work-item suggestion; an item with no type answers a null `type_id`. */
export const EntitySearchIssue = z.object({
  name: z.string(),
  id: uuid(),
  sequence_id: z.number(),
  project__identifier: z.string(),
  project_id: uuid(),
  priority: z.string(),
  state_id: uuid(),
  type_id: z.nullable(uuid()),
});
export type EntitySearchIssue = z.infer<typeof EntitySearchIssue>;

/** One cycle suggestion; `status` is derived from the cycle's dates. */
export const EntitySearchCycle = z.object({
  name: z.string(),
  id: uuid(),
  project_id: uuid(),
  project__identifier: z.string(),
  status: z.string(),
  workspace__slug: z.string(),
});
export type EntitySearchCycle = z.infer<typeof EntitySearchCycle>;

/** One module suggestion. */
export const EntitySearchModule = z.object({
  name: z.string(),
  id: uuid(),
  project_id: uuid(),
  project__identifier: z.string(),
  status: z.string(),
  workspace__slug: z.string(),
});
export type EntitySearchModule = z.infer<typeof EntitySearchModule>;

/**
 * One page suggestion: only open pages of the scoped project come back;
 * `projects__id` is the flat project id a page link token stores.
 */
export const EntitySearchPage = z.object({
  name: z.string(),
  id: uuid(),
  logo_props: z.record(z.string(), z.unknown()),
  projects__id: z.nullable(uuid()),
  workspace__slug: z.string(),
});
export type EntitySearchPage = z.infer<typeof EntitySearchPage>;

/**
 * The typed preview kinds a page body resolves: the trigger kinds the
 * editor understands today. People are the mention picker's section.
 */
export const ENTITY_PREVIEW_KINDS = ["project", "issue", "cycle", "module", "page"] as const;
export type EntityPreviewKind = (typeof ENTITY_PREVIEW_KINDS)[number];

/** One typed section per preview kind. */
export const EntityPreviewSections = z.object({
  project: z.optional(z.array(EntitySearchProject)),
  issue: z.optional(z.array(EntitySearchIssue)),
  cycle: z.optional(z.array(EntitySearchCycle)),
  module: z.optional(z.array(EntitySearchModule)),
  page: z.optional(z.array(EntitySearchPage)),
});
export type EntityPreviewSections = z.infer<typeof EntityPreviewSections>;

/** One parser per preview kind; the loop below picks by the kind it asked for. */
const ENTITY_PREVIEW_SECTION_SCHEMAS: Record<EntityPreviewKind, { parse(data: unknown): unknown }> = {
  project: z.array(EntitySearchProject),
  issue: z.array(EntitySearchIssue),
  cycle: z.array(EntitySearchCycle),
  module: z.array(EntitySearchModule),
  page: z.array(EntitySearchPage),
};

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

/**
 * Read the preview sections a page body can draw cards for. Each section is
 * parsed on its own, so a kind the server echoes but the caller did not ask
 * for can never fail the read.
 */
export async function searchEntityPreviews(
  client: ApiClient,
  slug: string,
  query: Omit<EntitySearchQuery, "queryTypes"> & { kinds?: readonly EntityPreviewKind[] | undefined }
): Promise<EntityPreviewSections> {
  const path = `/api/workspaces/${slug}/entity-search/`;
  const kinds = query.kinds ?? ENTITY_PREVIEW_KINDS;
  const body = await client.getJson<unknown>(path, {
    query: {
      query: query.query,
      query_type: kinds.join(","),
      project_id: query.projectId,
      count: query.count,
    },
  });
  const envelope = z.record(z.string(), z.unknown()).parse(body);
  const sections: EntityPreviewSections = {};
  for (const kind of kinds) {
    const section = envelope[kind];
    if (section !== undefined) {
      (sections as Record<string, unknown>)[kind] = client.parse(
        ENTITY_PREVIEW_SECTION_SCHEMAS[kind],
        section,
        `GET ${path}`
      );
    }
  }
  return sections;
}
