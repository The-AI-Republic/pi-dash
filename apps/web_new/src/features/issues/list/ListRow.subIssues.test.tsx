// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression for an expanded row's children staying fresh: the layout caches
// the first child list, but the row must keep observing its sub-issues query
// so a lifecycle invalidation refetches it (parity/specs/issues.md §2).
import { QueryClientProvider } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./InlinePropertyStrip.js", () => ({
  InlinePropertyStrip: () => <div data-testid="strip" />,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { createQueryClient } from "../../../core/query/index.js";
import { issueKeys } from "../api/keys.js";
import { ListRow, type ListRowProps } from "./ListRow.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const CHILD = "123e4567-e89b-12d3-a456-426614174003";

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  };
}

function props(overrides: Partial<ListRowProps> = {}): ListRowProps {
  return {
    workspaceSlug: "acme",
    projectId: PROJECT,
    projectIdentifier: "WEB",
    issue: issue({ sub_issues_count: 1 }),
    depth: 1,
    properties: {},
    flags: { hideCycle: false, hideModule: false },
    readOnly: false,
    states: [],
    stateNames: new Map(),
    assigneeNames: new Map(),
    labelNames: new Map(),
    cycleNames: new Map(),
    moduleNames: new Map(),
    openCycleIds: new Set(),
    estimateEnabled: false,
    highlightedId: null,
    onOpenPeek: vi.fn(),
    hrefFor: (row) => `/acme/browse/WEB-${row.sequence_id}`,
    ...overrides,
  };
}

function subIssueResponse(name: string) {
  return {
    sub_issues: [{ ...issue({ id: CHILD, name, parent_id: ISSUE, sequence_id: 43 }), state_group: null }],
    state_distribution: {},
  };
}

function jsonResponse(value: unknown) {
  return {
    status: 200,
    url: "https://api.test/",
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(value),
  };
}

beforeEach(() => {
  resetAppClient();
});

afterEach(() => resetAppClient());

describe("ListRow expanded sub-issues", () => {
  it("refetches cached expanded children when issue queries are invalidated", async () => {
    const child = issue({ id: CHILD, name: "Old child", parent_id: ISSUE, sequence_id: 43 });
    const calls: string[] = [];
    const childCalls = () => calls.filter((url) => url.includes("sub-issues/"));
    createAppClient("https://api.test", async (url) => {
      calls.push(url);
      // The row's menu hook reads beside the children read; only the
      // children URL drives the Old/Fresh payload this case pins.
      if (!url.includes("sub-issues/")) return jsonResponse([]);
      return jsonResponse(subIssueResponse(childCalls().length === 1 ? "Old child" : "Fresh child"));
    });
    const queryClient = createQueryClient();
    const onChildrenChange = vi.fn();

    render(
      <QueryClientProvider client={queryClient}>
        <ListRow
          {...props({
            onChildrenChange,
            selectionTree: { expanded: new Set([ISSUE]), children: new Map([[ISSUE, [child]]]) },
          })}
        />
      </QueryClientProvider>
    );

    await waitFor(() => expect(childCalls()).toHaveLength(1));
    await waitFor(() => expect(onChildrenChange).toHaveBeenCalled());
    onChildrenChange.mockClear();

    await act(async () => queryClient.invalidateQueries({ queryKey: issueKeys.all("acme") }));

    await waitFor(() => expect(childCalls()).toHaveLength(2));
    expect(onChildrenChange).toHaveBeenCalledWith(
      ISSUE,
      expect.arrayContaining([expect.objectContaining({ id: CHILD, name: "Fresh child" })])
    );
  });
});
