// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { RecentIssueData, RecentPageData, RecentProjectData } from "../lib/recentRows.js";
import { RecentIssueRow, RecentPageRow, RecentProjectRow } from "./RecentRows.js";

function minutesAgo(minutes: number): string {
  return new Date(Date.now() - minutes * 60_000).toISOString();
}

const VISITED_AT = minutesAgo(0);

function issue(overrides?: Partial<RecentIssueData>): RecentIssueData {
  return {
    kind: "issue",
    title: "Parity first issue",
    issueId: "issue-1",
    projectId: "project-1",
    sequenceId: 12,
    projectIdentifier: "ABC",
    stateId: "state-1",
    stateNameHint: null,
    priority: "high",
    assigneeCount: 4,
    assigneeNames: ["Ada Lovelace", "Grace Hopper"],
    ...overrides,
  };
}

describe("RecentIssueRow", () => {
  it("links the detail view and shows age plus full context", () => {
    const onPreview = vi.fn();
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue()}
        stateName="In progress"
        stateColor="#ff0000"
        fallbackIdentifier={null}
        onPreview={onPreview}
      />
    );
    const row = screen.getByTestId("recent-row");
    expect(row).toHaveAttribute("href", "/acme/projects/project-1/issues/issue-1");
    expect(row).toHaveAttribute("data-kind", "issue");
    expect(within(row).getByText("Parity first issue")).toBeInTheDocument();
    expect(within(row).getByText("ABC-12")).toBeInTheDocument();
    expect(within(row).getByText("In progress")).toBeInTheDocument();
    expect(within(row).getByText("High")).toBeInTheDocument();
    expect(within(row).getByText("just now")).toBeInTheDocument();
    expect(within(row).getByRole("img", { name: "4 assignees" })).toBeInTheDocument();
    expect(within(row).getByText("+1")).toBeInTheDocument();
    expect(fireEvent.click(row)).toBe(false);
    expect(onPreview).toHaveBeenCalledTimes(1);
  });

  it("tooltips the priority display label, never the raw enum", () => {
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue({ priority: "urgent" })}
        stateName={null}
        stateColor={null}
        fallbackIdentifier={null}
        onPreview={vi.fn()}
      />
    );
    expect(screen.getByText("Urgent").closest("[title]")).toHaveAttribute("title", "Urgent");
    expect(screen.queryByText("urgent")).not.toBeInTheDocument();
  });

  it("renders the status name without a color override", () => {
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue()}
        stateName="In progress"
        stateColor={null}
        fallbackIdentifier={null}
        onPreview={vi.fn()}
      />
    );
    expect(screen.getByText("In progress")).toBeInTheDocument();
  });

  it("falls back to the project list identifier", () => {
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue({ projectIdentifier: null })}
        stateName={null}
        stateColor={null}
        fallbackIdentifier="WEB"
        onPreview={vi.fn()}
      />
    );
    expect(screen.getByText("WEB-12")).toBeInTheDocument();
  });

  it("omits markers for missing or unknown context", () => {
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue({ projectIdentifier: null, sequenceId: null, priority: "critical", assigneeCount: 0 })}
        stateName={null}
        stateColor={null}
        fallbackIdentifier={null}
        onPreview={vi.fn()}
      />
    );
    const row = screen.getByTestId("recent-row");
    expect(within(row).getByText("Parity first issue")).toBeInTheDocument();
    expect(within(row).queryByText("ABC-12")).not.toBeInTheDocument();
    expect(within(row).queryByRole("img")).not.toBeInTheDocument();
    expect(screen.queryByText("critical")).not.toBeInTheDocument();
  });

  it("omits the priority marker without a priority", () => {
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue({ priority: null })}
        stateName={null}
        stateColor={null}
        fallbackIdentifier={null}
        onPreview={vi.fn()}
      />
    );
    expect(screen.queryByText("None")).not.toBeInTheDocument();
  });

  it("points project-less work items home", () => {
    render(
      <RecentIssueRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        issue={issue({ projectId: null })}
        stateName={null}
        stateColor={null}
        fallbackIdentifier={null}
        onPreview={vi.fn()}
      />
    );
    expect(screen.getByTestId("recent-row")).toHaveAttribute("href", "/acme/");
  });
});

describe("RecentPageRow", () => {
  function page(overrides?: Partial<RecentPageData>): RecentPageData {
    return {
      kind: "page",
      title: "Launch notes",
      pageId: "page-1",
      projectId: "project-1",
      projectIdentifier: "ABC",
      hasOwner: true,
      ...overrides,
    };
  }

  it("navigates project pages inside the project", () => {
    const onOpen = vi.fn();
    render(<RecentPageRow workspaceSlug="acme" visitedAt={VISITED_AT} page={page()} onOpen={onOpen} />);
    const row = screen.getByTestId("recent-row");
    expect(row).toHaveAttribute("href", "/acme/projects/project-1/pages/page-1");
    expect(within(row).getByText("Launch notes")).toBeInTheDocument();
    expect(within(row).getByText("ABC")).toBeInTheDocument();
    expect(within(row).getByRole("img", { name: "1 owner" })).toBeInTheDocument();
    expect(fireEvent.click(row)).toBe(false);
    expect(onOpen).toHaveBeenCalledWith("/acme/projects/project-1/pages/page-1");
  });

  it("lands workspace pages on the workspace scope", () => {
    const onOpen = vi.fn();
    render(
      <RecentPageRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        page={page({ projectId: null, projectIdentifier: null, hasOwner: false })}
        onOpen={onOpen}
      />
    );
    const row = screen.getByTestId("recent-row");
    expect(row).toHaveAttribute("href", "/acme/pages/page-1");
    expect(within(row).queryByRole("img")).not.toBeInTheDocument();
    fireEvent.click(row);
    expect(onOpen).toHaveBeenCalledWith("/acme/pages/page-1");
  });
});

describe("RecentProjectRow", () => {
  function project(overrides?: Partial<RecentProjectData>): RecentProjectData {
    return {
      kind: "project",
      title: "Website",
      projectId: "project-1",
      identifier: "WEB",
      memberCount: 1,
      ...overrides,
    };
  }

  it("navigates to the project work list with its members", () => {
    const onOpen = vi.fn();
    render(<RecentProjectRow workspaceSlug="acme" visitedAt={VISITED_AT} project={project()} onOpen={onOpen} />);
    const row = screen.getByTestId("recent-row");
    expect(row).toHaveAttribute("href", "/acme/projects/project-1/issues");
    expect(within(row).getByText("Website")).toBeInTheDocument();
    expect(within(row).getByText("WEB")).toBeInTheDocument();
    expect(within(row).getByRole("img", { name: "1 member" })).toBeInTheDocument();
    expect(fireEvent.click(row)).toBe(false);
    expect(onOpen).toHaveBeenCalledWith("/acme/projects/project-1/issues");
  });

  it("omits the identifier and members when missing", () => {
    render(
      <RecentProjectRow
        workspaceSlug="acme"
        visitedAt={VISITED_AT}
        project={project({ identifier: null, memberCount: 0 })}
        onOpen={vi.fn()}
      />
    );
    const row = screen.getByTestId("recent-row");
    expect(within(row).queryByText("WEB")).not.toBeInTheDocument();
    expect(within(row).queryByRole("img")).not.toBeInTheDocument();
  });
});
