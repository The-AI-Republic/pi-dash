// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public board issue reads (parity/specs/space.md §5.1): the grouped list
// the board renders (`group_by=state_id`, one bucket per workflow state)
// and the single-issue detail the peek panel opens. Both payloads are
// flat `.values()` rows — there is no nested state/project/votes object;
// engagement rides in `vote_items`/`reaction_items`.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { nullableDate, nullableUuid, uuid } from "./fields.js";

/** Workflow-state groups the public board can render. */
export const SPACE_ISSUE_STATE_GROUPS = [
  "backlog",
  "unstarted",
  "started",
  "review",
  "test",
  "completed",
  "cancelled",
  "triage",
] as const;

export const SPACE_ISSUE_PRIORITIES = ["urgent", "high", "medium", "low", "none"] as const;

/** Actor projection embedded in an issue's vote/reaction items. */
export const SpaceEngagementActor = z.object({
  id: uuid(),
  display_name: z.string(),
  first_name: z.string(),
  last_name: z.string(),
  avatar: z.string(),
  avatar_url: z.nullable(z.string()),
});
export type SpaceEngagementActor = z.infer<typeof SpaceEngagementActor>;

/** One vote as embedded on a board issue row. */
export const SpaceIssueVoteItem = z.object({
  vote: z.number(),
  actor_details: SpaceEngagementActor,
});
export type SpaceIssueVoteItem = z.infer<typeof SpaceIssueVoteItem>;

/** One emoji reaction as embedded on a board issue row. */
export const SpaceIssueReactionItem = z.object({
  reaction: z.string(),
  actor_details: SpaceEngagementActor,
});
export type SpaceIssueReactionItem = z.infer<typeof SpaceIssueReactionItem>;

/**
 * One row in the public board's issue list. Deliberately flat: `state_id`
 * plus `state__group` describe the state, `label_ids`/`assignee_ids`/
 * `module_ids` are bare id arrays, and the card reads the embedded
 * engagement arrays rather than a separate votes/reactions list.
 */
export const SpaceBoardIssue = z.object({
  id: uuid(),
  name: z.string(),
  state_id: nullableUuid(),
  sort_order: z.number(),
  estimate_point: z.nullable(z.union([uuid(), z.number(), z.string()])),
  priority: z.nullable(z.enum(SPACE_ISSUE_PRIORITIES)),
  start_date: nullableDate(),
  target_date: nullableDate(),
  sequence_id: z.number(),
  project_id: uuid(),
  parent_id: nullableUuid(),
  created_by: nullableUuid(),
  state__group: z.nullable(z.enum(SPACE_ISSUE_STATE_GROUPS)),
  cycle_id: nullableUuid(),
  assignee_ids: z.array(uuid()),
  label_ids: z.array(uuid()),
  module_ids: z.array(uuid()),
  vote_items: z.array(SpaceIssueVoteItem),
  reaction_items: z.array(SpaceIssueReactionItem),
});
export type SpaceBoardIssue = z.infer<typeof SpaceBoardIssue>;

/**
 * The detail shape: the list row plus the four description fields. The
 * peek panel renders `description_html` (falling back to the stripped
 * text) and keeps the JSON for the editor.
 */
export const SpaceBoardIssueDetail = z.extend(SpaceBoardIssue, {
  description_json: z.unknown(),
  description_html: z.string(),
  description_stripped: z.nullable(z.string()),
  description_binary: z.nullable(z.unknown()),
});
export type SpaceBoardIssueDetail = z.infer<typeof SpaceBoardIssueDetail>;

/** One state bucket in a grouped page. */
export const SpaceBoardIssueGroup = z.object({
  results: z.array(SpaceBoardIssue),
  total_results: z.number(),
});
export type SpaceBoardIssueGroup = z.infer<typeof SpaceBoardIssueGroup>;

/**
 * The grouped list envelope. `results` is keyed by state id when
 * `grouped_by` is `state_id`; the other top-level counters come from the
 * shared offset paginator.
 */
export const SpaceBoardIssuePage = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.optional(z.nullable(z.string())),
  total_count: z.number(),
  next_cursor: z.nullable(z.string()),
  prev_cursor: z.optional(z.nullable(z.string())),
  next_page_results: z.boolean(),
  prev_page_results: z.optional(z.boolean()),
  count: z.optional(z.number()),
  total_pages: z.optional(z.number()),
  total_results: z.optional(z.number()),
  extra_stats: z.optional(z.unknown()),
  results: z.record(z.string(), SpaceBoardIssueGroup),
});
export type SpaceBoardIssuePage = z.infer<typeof SpaceBoardIssuePage>;

/** Query parameters the board sends for its grouped list. */
export interface SpaceBoardIssueQuery {
  /** Always `state_id` for the board's column/group layout. */
  groupBy?: "state_id";
  /** Page size (the board sends 50). */
  perPage?: number;
  /** Opaque group cursor; first request is the literal `50:0:0`. */
  cursor?: string;
  orderBy?: string;
  /** Workflow state ids to include. */
  state?: string[];
  priority?: string[];
  /** Label ids, joined with commas on the wire. */
  labels?: string[];
}

function joinParams(values: string[] | undefined): string | undefined {
  return values && values.length > 0 ? values.join(",") : undefined;
}

export async function listSpaceBoardIssues(
  client: ApiClient,
  anchor: string,
  query: SpaceBoardIssueQuery = {}
): Promise<SpaceBoardIssuePage> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/`;
  const body = await client.getJson<unknown>(path, {
    query: {
      group_by: query.groupBy,
      per_page: query.perPage,
      cursor: query.cursor,
      order_by: query.orderBy,
      state: joinParams(query.state),
      priority: joinParams(query.priority),
      labels: joinParams(query.labels),
    },
  });
  return client.parse(SpaceBoardIssuePage, body, `GET ${path}`);
}

export async function getSpaceBoardIssue(
  client: ApiClient,
  anchor: string,
  issueId: string
): Promise<SpaceBoardIssueDetail> {
  const path = `/api/public/anchor/${encodeURIComponent(anchor)}/issues/${encodeURIComponent(issueId)}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(SpaceBoardIssueDetail, body, `GET ${path}`);
}
