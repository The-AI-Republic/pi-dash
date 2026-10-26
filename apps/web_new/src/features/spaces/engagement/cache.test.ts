// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, type QueryKey } from "@tanstack/react-query";
import type { SpaceBoardIssue, SpaceBoardIssueDetail, SpaceBoardIssuePage } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { spaceKeys } from "../api/keys.js";
import {
  applySpaceEngagement,
  invalidateSpaceEngagement,
  isSpaceBoardPageKey,
  restoreSpaceEngagement,
  snapshotSpaceEngagement,
} from "./cache.js";

const ANCHOR = "anchor-1";
const ISSUE_ID = "11111111-1111-4111-8111-111111111111";
const OTHER_ID = "22222222-2222-4222-8222-222222222222";
const STATE_ID = "33333333-3333-4333-8333-333333333333";

function issue(overrides: Partial<SpaceBoardIssue> = {}): SpaceBoardIssue {
  return {
    id: ISSUE_ID,
    name: "Public work item",
    state_id: STATE_ID,
    sort_order: 1,
    estimate_point: null,
    priority: null,
    start_date: null,
    target_date: null,
    sequence_id: 7,
    project_id: "44444444-4444-4444-8444-444444444444",
    parent_id: null,
    created_by: null,
    state__group: "unstarted",
    cycle_id: null,
    assignee_ids: [],
    label_ids: [],
    module_ids: [],
    vote_items: [],
    reaction_items: [],
    ...overrides,
  };
}

function detail(overrides: Partial<SpaceBoardIssueDetail> = {}): SpaceBoardIssueDetail {
  return {
    ...issue(),
    description_json: null,
    description_html: "",
    description_stripped: null,
    description_binary: null,
    ...overrides,
  };
}

function page(rows: SpaceBoardIssue[]): SpaceBoardIssuePage {
  return {
    grouped_by: "state_id",
    total_count: rows.length,
    next_cursor: null,
    next_page_results: false,
    results: { [STATE_ID]: { results: rows, total_results: rows.length } },
  };
}

const ADDED_VOTE = {
  vote: 1,
  actor_details: {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    display_name: "Ada",
    first_name: "Ada",
    last_name: "",
    avatar: "",
    avatar_url: null,
  },
};

describe("isSpaceBoardPageKey", () => {
  it("matches first and later list pages of one board only", () => {
    expect(isSpaceBoardPageKey(ANCHOR, spaceKeys.issues(ANCHOR, "sig"))).toBe(true);
    expect(isSpaceBoardPageKey(ANCHOR, [...spaceKeys.group(ANCHOR, STATE_ID, "sig"), "cursor"])).toBe(true);
    expect(isSpaceBoardPageKey(ANCHOR, spaceKeys.detail(ANCHOR, ISSUE_ID))).toBe(false);
    expect(isSpaceBoardPageKey(ANCHOR, spaceKeys.issues("anchor-2", "sig"))).toBe(false);
    expect(isSpaceBoardPageKey(ANCHOR, ["spaces", "anchor"])).toBe(false);
    expect(isSpaceBoardPageKey(ANCHOR, "spaces" as unknown as QueryKey)).toBe(false);
  });
});

describe("snapshot/apply/restoreSpaceEngagement", () => {
  it("patches the detail and every cached list page, then rolls back", () => {
    const queryClient = new QueryClient();
    const firstKey = spaceKeys.issues(ANCHOR, "sig");
    const groupKey = [...spaceKeys.group(ANCHOR, STATE_ID, "sig"), "cursor"] as unknown as QueryKey;
    queryClient.setQueryData(spaceKeys.detail(ANCHOR, ISSUE_ID), detail());
    queryClient.setQueryData(firstKey, page([issue()]));
    queryClient.setQueryData(groupKey, page([issue(), issue({ id: OTHER_ID })]));
    queryClient.setQueryData(spaceKeys.issues("anchor-2", "sig"), page([issue()]));

    const snapshot = snapshotSpaceEngagement(queryClient, ANCHOR, ISSUE_ID);
    expect(snapshot.detail?.id).toBe(ISSUE_ID);
    expect(snapshot.pages).toHaveLength(2);

    applySpaceEngagement(queryClient, ANCHOR, ISSUE_ID, (row) => ({ ...row, vote_items: [ADDED_VOTE] }));
    expect(queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID))?.vote_items).toEqual([
      ADDED_VOTE,
    ]);
    expect(queryClient.getQueryData<SpaceBoardIssuePage>(firstKey)?.results[STATE_ID]?.results[0]?.vote_items).toEqual([
      ADDED_VOTE,
    ]);
    const groupRows = queryClient.getQueryData<SpaceBoardIssuePage>(groupKey)?.results[STATE_ID]?.results ?? [];
    expect(groupRows[0]?.vote_items).toEqual([ADDED_VOTE]);
    expect(groupRows[1]?.vote_items).toEqual([]);
    // Another board's page is untouched.
    expect(
      queryClient.getQueryData<SpaceBoardIssuePage>(spaceKeys.issues("anchor-2", "sig"))?.results[STATE_ID]?.results[0]
        ?.vote_items
    ).toEqual([]);

    restoreSpaceEngagement(queryClient, snapshot);
    expect(queryClient.getQueryData<SpaceBoardIssueDetail>(spaceKeys.detail(ANCHOR, ISSUE_ID))?.vote_items).toEqual([]);
    expect(queryClient.getQueryData<SpaceBoardIssuePage>(firstKey)?.results[STATE_ID]?.results[0]?.vote_items).toEqual(
      []
    );
  });

  it("leaves missing copies alone", () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(spaceKeys.issues(ANCHOR, "sig"), page([issue({ id: OTHER_ID })]));
    const snapshot = snapshotSpaceEngagement(queryClient, ANCHOR, ISSUE_ID);
    expect(snapshot.detail).toBeUndefined();
    const before = queryClient.getQueryData(spaceKeys.issues(ANCHOR, "sig"));
    applySpaceEngagement(queryClient, ANCHOR, ISSUE_ID, (row) => ({ ...row, vote_items: [ADDED_VOTE] }));
    expect(queryClient.getQueryData(spaceKeys.detail(ANCHOR, ISSUE_ID))).toBeUndefined();
    expect(queryClient.getQueryData(spaceKeys.issues(ANCHOR, "sig"))).toBe(before);
    restoreSpaceEngagement(queryClient, snapshot);
    expect(queryClient.getQueryData(spaceKeys.issues(ANCHOR, "sig"))).toBe(before);
  });

  it("tolerates cached queries that hold no page yet", () => {
    const queryClient = new QueryClient();
    // A mounted but still pending list query: an entry with no data.
    queryClient.getQueryCache().build(queryClient, {
      queryKey: spaceKeys.issues(ANCHOR, "sig"),
      queryFn: () => new Promise<SpaceBoardIssuePage>(() => undefined),
    });
    const snapshot = snapshotSpaceEngagement(queryClient, ANCHOR, ISSUE_ID);
    expect(snapshot.pages).toHaveLength(1);
    applySpaceEngagement(queryClient, ANCHOR, ISSUE_ID, (row) => ({ ...row, vote_items: [ADDED_VOTE] }));
    expect(queryClient.getQueryData(spaceKeys.issues(ANCHOR, "sig"))).toBeUndefined();
    restoreSpaceEngagement(queryClient, snapshot);
    expect(queryClient.getQueryData(spaceKeys.issues(ANCHOR, "sig"))).toBeUndefined();
  });
});

describe("invalidateSpaceEngagement", () => {
  it("marks the detail and the board's pages stale", async () => {
    const queryClient = new QueryClient();
    const detailKey = spaceKeys.detail(ANCHOR, ISSUE_ID);
    const pageKey = spaceKeys.issues(ANCHOR, "sig");
    const otherKey = spaceKeys.issues("anchor-2", "sig");
    queryClient.setQueryData(detailKey, detail());
    queryClient.setQueryData(pageKey, page([issue()]));
    queryClient.setQueryData(otherKey, page([issue()]));
    await invalidateSpaceEngagement(queryClient, ANCHOR, ISSUE_ID);
    expect(queryClient.getQueryState(detailKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(pageKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(otherKey)?.isInvalidated).toBe(false);
  });
});
