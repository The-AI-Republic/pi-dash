// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Request detail read (INT-019/028/031): key, no-refetch policy, the
// work-item adapter and the missing-id classifier.

import { QueryClient } from "@tanstack/react-query";
import { ApiError, type IntakeIssueDetail, type IntakeIssueDetailItem } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { intakeDetailQueryOptions, intakeDetailWorkItem, isMissingIntakeDetail } from "./detail.js";
import { intakeKeys } from "./keys.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "11111111-1111-4111-8111-111111111111";

function detailItem(overrides: Partial<IntakeIssueDetailItem> = {}): IntakeIssueDetailItem {
  return {
    id: ISSUE,
    name: "A request",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>Body</p>",
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

/** A client whose JSON GET answers `body`. */
function fakeClient(body: unknown) {
  return {
    getJson: async () => body,
    parse: (_schema: unknown, value: unknown) => value,
  } as never;
}

describe("intakeDetailQueryOptions (INT-031)", () => {
  it("reads the request once under the workspace scope with no refetch", async () => {
    const detail: IntakeIssueDetail = {
      id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
      status: -2,
      duplicate_to: null,
      snoozed_till: null,
      duplicate_issue_detail: null,
      source: null,
      issue: detailItem(),
    };
    const options = intakeDetailQueryOptions(fakeClient(detail), WS, PROJECT, ISSUE);
    expect(options.queryKey.slice(0, 2)).toEqual(["ws", WS]);
    expect(options.queryKey).toEqual(intakeKeys.detail(WS, PROJECT, ISSUE));
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
    expect(options.refetchInterval).toBe(false);

    const client = new QueryClient();
    try {
      await expect(client.fetchQuery({ ...options, retry: false })).resolves.toEqual(detail);
    } finally {
      client.clear();
    }
  });
});

describe("intakeDetailWorkItem", () => {
  it("restores the keys the intake viewset never annotates", () => {
    const adapted = intakeDetailWorkItem(detailItem());
    expect(adapted.cycle_id).toBeNull();
    expect(adapted.module_ids).toEqual([]);
    expect(adapted.sub_issues_count).toBeNull();
    expect(adapted.attachment_count).toBeNull();
    expect(adapted.link_count).toBeNull();
    expect(adapted.is_subscribed).toBe(false);
    expect(adapted.name).toBe("A request");
  });

  it("keeps annotated values when the serializer sends them", () => {
    const adapted = intakeDetailWorkItem(
      detailItem({
        cycle_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        module_ids: ["dddddddd-dddd-4ddd-8ddd-dddddddddddd"],
        sub_issues_count: 2,
        attachment_count: 1,
        link_count: 3,
        is_subscribed: true,
      })
    );
    expect(adapted.cycle_id).toBe("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    expect(adapted.module_ids).toEqual(["dddddddd-dddd-4ddd-8ddd-dddddddddddd"]);
    expect(adapted.sub_issues_count).toBe(2);
    expect(adapted.attachment_count).toBe(1);
    expect(adapted.link_count).toBe(3);
    expect(adapted.is_subscribed).toBe(true);
  });
});

describe("isMissingIntakeDetail", () => {
  it.each([404, 403])("treats status %i as a stale or forbidden id", (status) => {
    expect(isMissingIntakeDetail(new ApiError({ status, code: "http", message: "gone" }))).toBe(true);
  });

  it("treats other failures as errors worth a retry", () => {
    expect(isMissingIntakeDetail(new ApiError({ status: 500, code: "http", message: "boom" }))).toBe(false);
    expect(isMissingIntakeDetail(new ApiError({ code: "network", message: "offline" }))).toBe(false);
    expect(isMissingIntakeDetail(new Error("plain"))).toBe(false);
    expect(isMissingIntakeDetail(null)).toBe(false);
  });
});
