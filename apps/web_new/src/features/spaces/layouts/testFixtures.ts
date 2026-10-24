// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared shapes for the public-board layout tests (test-only module).
import type { SpaceBoardIssue, SpaceBoardIssuePage, SpaceBoardState } from "@pidash/api-client";

export const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
export const TODO_ID = "22222222-2222-4222-8222-222222222222";
export const DONE_ID = "33333333-3333-4333-8333-333333333333";
export const EMPTY_ID = "44444444-4444-4444-8444-444444444444";

export const STATES: SpaceBoardState[] = [
  { id: DONE_ID, name: "Done", color: "#0a0", group: "completed", sequence: 3 },
  { id: EMPTY_ID, name: "Empty", color: "#00a", group: "backlog", sequence: 2 },
  { id: TODO_ID, name: "Todo", color: "#a00", group: "unstarted", sequence: 1 },
];

let nextId = 0;

/** A publishable board row with a unique id per call. */
export function boardIssue(sequenceId: number, stateId = TODO_ID, name = `Issue ${sequenceId}`): SpaceBoardIssue {
  nextId += 1;
  return {
    id: `${String(nextId).padStart(8, "0")}-0000-4000-8000-000000000000`,
    name,
    state_id: stateId,
    sort_order: sequenceId,
    estimate_point: null,
    priority: null,
    start_date: null,
    target_date: null,
    sequence_id: sequenceId,
    project_id: PROJECT_ID,
    parent_id: null,
    created_by: null,
    state__group: "unstarted",
    cycle_id: null,
    assignee_ids: [],
    label_ids: [],
    module_ids: [],
    vote_items: [],
    reaction_items: [],
  };
}

export function firstPage(
  buckets: Record<string, { results: SpaceBoardIssue[]; total_results: number }>,
  overrides: Partial<SpaceBoardIssuePage> = {}
): SpaceBoardIssuePage {
  const total = Object.values(buckets).reduce((sum, bucket) => sum + bucket.results.length, 0);
  return {
    grouped_by: "state_id",
    total_count: total,
    next_cursor: null,
    next_page_results: false,
    results: buckets,
    ...overrides,
  };
}
