// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { WorkspaceMember } from "@pidash/api-client";

import { freshListFilters, type ListFilters } from "../lib/list-display.js";
import { ProjectsAppliedFilters } from "./ProjectsAppliedFilters.js";

function member(id: string, displayName: string): WorkspaceMember {
  return {
    id: `m-${id}`,
    member: {
      id,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: "",
      display_name: displayName,
      is_bot: false,
    },
    role: 10,
  };
}

const MEMBERS = [member("u1", "Ada Lovelace"), member("u2", "Grace Hopper")];

function setup(
  filters: ListFilters,
  counts: { filteredCount: number; totalCount: number } = { filteredCount: 1, totalCount: 2 },
  members: WorkspaceMember[] = MEMBERS
) {
  const handlers = {
    onRemoveMyProjects: vi.fn(),
    onRemoveValue: vi.fn(),
    onRemoveDimension: vi.fn(),
    onClearAll: vi.fn(),
  };
  render(
    <ProjectsAppliedFilters
      filters={filters}
      members={members}
      filteredCount={counts.filteredCount}
      totalCount={counts.totalCount}
      onRemoveMyProjects={handlers.onRemoveMyProjects}
      onRemoveValue={handlers.onRemoveValue}
      onRemoveDimension={handlers.onRemoveDimension}
      onClearAll={handlers.onClearAll}
    />
  );
  return handlers;
}

describe("ProjectsAppliedFilters", () => {
  it("hides without narrowing filters", () => {
    setup(freshListFilters());
    expect(screen.queryByTestId("projects-applied-filters")).toBeNull();
  });

  it("pills access picks with the match count and clears", () => {
    const handlers = setup({ ...freshListFilters(), access: [0, 2], myProjects: true });

    expect(screen.getByText("Private")).not.toBeNull();
    expect(screen.getByText("My projects")).not.toBeNull();
    expect(screen.getByTestId("projects-filter-count")).toHaveTextContent("1/2");
    expect(screen.getByTestId("projects-filter-count")).toHaveAttribute(
      "title",
      "1 of 2 projects match the applied filters."
    );

    fireEvent.click(screen.getByRole("button", { name: "Remove Private" }));
    expect(handlers.onRemoveValue).toHaveBeenCalledWith("access", "0");

    fireEvent.click(screen.getByRole("button", { name: "Clear Access filters" }));
    expect(handlers.onRemoveDimension).toHaveBeenCalledWith("access");

    fireEvent.click(screen.getByRole("button", { name: "Remove My projects" }));
    expect(handlers.onRemoveMyProjects).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(handlers.onClearAll).toHaveBeenCalledTimes(1);
  });

  it("pills people by display name and drops unknown ids", () => {
    const handlers = setup({ ...freshListFilters(), lead: ["u1", "ghost"], members: ["u2"] });

    expect(screen.getByText("Ada Lovelace")).not.toBeNull();
    expect(screen.getByText("Grace Hopper")).not.toBeNull();
    expect(screen.queryByText("ghost")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Remove Ada Lovelace" }));
    expect(handlers.onRemoveValue).toHaveBeenCalledWith("lead", "u1");

    fireEvent.click(screen.getByRole("button", { name: "Clear Members filters" }));
    expect(handlers.onRemoveDimension).toHaveBeenCalledWith("members");
  });

  it("pills creation dates as presets or day bounds", () => {
    setup({ ...freshListFilters(), createdAt: ["last_7_days;custom;custom"] });
    expect(screen.getByText("Last 7 days")).not.toBeNull();
  });

  it("pills a custom range as its two bounds", () => {
    const handlers = setup({ ...freshListFilters(), createdAt: ["2026-10-01;after", "2026-10-31;before"] });

    expect(screen.getByText("After Oct 01, 2026")).not.toBeNull();
    expect(screen.getByText("Before Oct 31, 2026")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Remove After Oct 01, 2026" }));
    expect(handlers.onRemoveValue).toHaveBeenCalledWith("createdAt", "2026-10-01;after");
  });

  it("pills people who never set an avatar", () => {
    const noAvatar = member("u3", "No Avatar");
    noAvatar.member.avatar_url = null;
    setup({ ...freshListFilters(), lead: ["u3"] }, undefined, [noAvatar]);

    expect(screen.getByText("No Avatar")).not.toBeNull();
  });

  it("omits groups whose values cannot label", () => {
    setup({ ...freshListFilters(), access: [99], lead: ["ghost"], createdAt: ["bogus"] });

    expect(screen.getByTestId("projects-applied-filters")).not.toBeNull();
    expect(screen.queryByText("Access")).toBeNull();
    expect(screen.queryByText("Lead")).toBeNull();
    expect(screen.queryByText("Created date")).toBeNull();
    expect(screen.getByRole("button", { name: "Clear all" })).not.toBeNull();
  });
});
