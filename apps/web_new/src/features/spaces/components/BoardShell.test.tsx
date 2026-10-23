// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardSettings } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { BoardLoading, BoardNavbar, issueKey } from "./BoardShell.js";

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

describe("issueKey", () => {
  it("builds keys without a project identifier", () => {
    expect(issueKey("", { sequence_id: 3 })).toBe("-3");
    expect(issueKey("SPC", { sequence_id: 7 })).toBe("SPC-7");
  });
});

describe("BoardLoading", () => {
  it("announces the board fetch", () => {
    render(<BoardLoading />);
    expect(screen.getByTestId("space-board-loading")).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Loading board" })).toBeInTheDocument();
  });
});
