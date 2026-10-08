// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import type { IssueListItem } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { IssueList } from "./IssueList.js";

// jsdom reports zero element sizes, so the virtualizer would compute an
// empty window. Giving the scroll region and rows a height lets the rows
// under test mount; anything else keeps the original getters.
const realHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      if (this.getAttribute("role") === "region") return 400;
      if (this.hasAttribute("data-index")) return 44;
      return realHeight?.get?.call(this) ?? 0;
    },
  });
});

afterEach(() => {
  if (realHeight) Object.defineProperty(HTMLElement.prototype, "offsetHeight", realHeight);
});

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function row(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: UUID,
    name: "Fix login",
    state_id: UUID2,
    sort_order: 1,
    completed_at: null,
    priority: "high",
    sequence_id: 42,
    project_id: UUID2,
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

describe("IssueList", () => {
  it("renders one row per issue with its key and priority", () => {
    render(
      <IssueList
        issues={[row(), row({ id: UUID2, name: "Second", sequence_id: 43, priority: "none" })]}
        projectIdentifier="WEB"
        layout="list"
      />
    );
    expect(screen.getByLabelText("WEB-42")).toHaveTextContent("Fix login");
    expect(screen.getByLabelText("WEB-43")).toHaveTextContent("Second");
    expect(screen.getByText("high")).toBeInTheDocument();
  });

  it("renders the empty state without rows", () => {
    render(<IssueList issues={[]} projectIdentifier="WEB" layout="list" />);
    expect(screen.getByText("No issues yet")).toBeInTheDocument();
    expect(screen.queryByRole("article")).not.toBeInTheDocument();
  });
});
