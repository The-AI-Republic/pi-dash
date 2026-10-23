// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardIssuePage, SpaceBoardSettings } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { BoardIssues, BoardLoading, BoardNavbar, issueKey } from "./BoardShell.js";

const STATE_ID = "22222222-2222-4222-8222-222222222222";
const ISSUE_ID = "33333333-3333-4333-8333-333333333333";

function settingsWith(logo: Record<string, unknown>, name = "Public board"): SpaceBoardSettings {
  return {
    id: "44444444-4444-4444-8444-444444444444",
    project_details: {
      id: "11111111-1111-4111-8111-111111111111",
      identifier: "SPC",
      name,
      cover_image: null,
      cover_image_url: null,
      logo_props: logo,
      description: "",
      is_default: false,
    },
    workspace_detail: { name: "Acme", slug: "acme", id: "11111111-1111-4111-8111-111111111111", logo_url: null },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
    created_by: null,
    updated_by: null,
    workspace: "11111111-1111-4111-8111-111111111111",
    project: "11111111-1111-4111-8111-111111111111",
    entity_identifier: "project",
    entity_name: "project",
    anchor: "anchor-1",
    is_comments_enabled: false,
    is_reactions_enabled: false,
    is_votes_enabled: false,
    view_props: {},
    is_disabled: false,
    intake: null,
  };
}

const page: SpaceBoardIssuePage = {
  grouped_by: "state_id",
  total_count: 1,
  next_cursor: null,
  next_page_results: false,
  results: {
    [STATE_ID]: {
      results: [
        {
          id: ISSUE_ID,
          name: "Read this issue",
          state_id: STATE_ID,
          sort_order: 1,
          estimate_point: null,
          priority: null,
          start_date: null,
          target_date: null,
          sequence_id: 7,
          project_id: "11111111-1111-4111-8111-111111111111",
          parent_id: null,
          created_by: null,
          state__group: "unstarted",
          cycle_id: null,
          assignee_ids: [],
          label_ids: [],
          module_ids: [],
          vote_items: [],
          reaction_items: [],
        },
      ],
      total_results: 1,
    },
  },
};

describe("BoardNavbar", () => {
  it("shows placeholder chrome without inventing a mark", () => {
    render(<BoardNavbar settings={undefined} anchor="anchor-1" search={{}} />);
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("...");
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("");
  });

  it("renders an emoji mark and the project name", () => {
    render(
      <BoardNavbar
        settings={settingsWith({ in_use: "emoji", emoji: { value: "128512" } })}
        anchor="anchor-1"
        search={{}}
      />
    );
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("😀");
    expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board");
  });

  it("falls back to the project initial when the mark is not an emoji", () => {
    render(
      <BoardNavbar settings={settingsWith({ in_use: "icon", icon: { name: "rocket" } })} anchor="a" search={{}} />
    );
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("P");
  });

  it("keeps the position in the sign-in link and lets later slices fill slots", () => {
    const { rerender } = render(
      <BoardNavbar settings={settingsWith({})} anchor="anchor-1" search={{ board: "kanban", peekId: "p1" }} />
    );
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute(
      "href",
      "/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dkanban%26peekId%3Dp1"
    );
    rerender(
      <BoardNavbar settings={settingsWith({})} anchor="anchor-1" search={{}} accountSlot={<span>account slot</span>}>
        <span>navbar child</span>
      </BoardNavbar>
    );
    expect(screen.getByText("account slot")).toBeInTheDocument();
    expect(screen.getByText("navbar child")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Sign in" })).not.toBeInTheDocument();
  });
});

describe("BoardIssues", () => {
  it("groups rows by state and builds the public key from settings", () => {
    render(
      <BoardIssues
        page={page}
        states={[{ id: STATE_ID, name: "Todo", color: "#000", group: "unstarted", sequence: 1 }]}
        projectIdentifier="SPC"
      />
    );
    expect(screen.getByRole("heading", { name: "Todo" })).toBeInTheDocument();
    expect(screen.getByText("Read this issue")).toBeInTheDocument();
    expect(screen.getByText("SPC-7")).toBeInTheDocument();
  });

  it("labels a group whose state is missing without failing the board", () => {
    render(<BoardIssues page={page} states={[]} projectIdentifier="SPC" />);
    expect(screen.getByRole("heading", { name: "Issues" })).toBeInTheDocument();
  });

  it("renders nothing for empty groups", () => {
    render(
      <BoardIssues
        page={{ ...page, results: { [STATE_ID]: { results: [], total_results: 0 } } }}
        states={[]}
        projectIdentifier="SPC"
      />
    );
    expect(screen.queryByRole("heading")).not.toBeInTheDocument();
  });

  it("builds keys without a project identifier", () => {
    expect(issueKey("", { sequence_id: 3 })).toBe("-3");
  });
});

describe("BoardLoading", () => {
  it("announces the board fetch", () => {
    render(<BoardLoading />);
    expect(screen.getByTestId("space-board-loading")).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Loading board" })).toBeInTheDocument();
  });
});
