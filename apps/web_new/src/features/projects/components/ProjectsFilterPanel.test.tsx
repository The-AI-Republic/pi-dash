// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { WorkspaceMember } from "@pidash/api-client";

import { freshListFilters, type ListFilters } from "../lib/list-display.js";
import { ProjectsFilterPanel } from "./ProjectsFilterPanel.js";

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

const MEMBERS = [
  member("u1", "Ada Lovelace"),
  member("u2", "Grace Hopper"),
  member("u3", "Alan Turing"),
  member("u4", "Barbara Liskov"),
  member("u5", "Edsger Dijkstra"),
  member("u6", "Frances Allen"),
];

interface Handlers {
  onToggleMyProjects: () => void;
  onToggleAccess: (value: number) => void;
  onTogglePerson: (dimension: "lead" | "members", id: string) => void;
  onSelectCreated: (values: string[]) => void;
}

async function setup(
  args: {
    filters?: ListFilters;
    members?: WorkspaceMember[] | undefined;
    selfId?: string | null;
    handlers?: Partial<Handlers>;
  } = {}
) {
  const user = userEvent.setup();
  const handlers: Handlers = {
    onToggleMyProjects: vi.fn(),
    onToggleAccess: vi.fn(),
    onTogglePerson: vi.fn(),
    onSelectCreated: vi.fn(),
    ...args.handlers,
  };
  render(
    <ProjectsFilterPanel
      filters={args.filters ?? freshListFilters()}
      members={"members" in args ? args.members : MEMBERS}
      selfId={args.selfId === undefined ? "u1" : args.selfId}
      onToggleMyProjects={handlers.onToggleMyProjects}
      onToggleAccess={handlers.onToggleAccess}
      onTogglePerson={handlers.onTogglePerson}
      onSelectCreated={handlers.onSelectCreated}
    />
  );
  // The popover opens through the real pointer path; fireEvent skips the
  // press bookkeeping Base UI needs, so the whole suite drives user-event.
  await user.click(screen.getByTestId("projects-filter-trigger"));
  return { user, handlers };
}

function badgeOnly(filters: ListFilters) {
  render(
    <ProjectsFilterPanel
      filters={filters}
      members={MEMBERS}
      selfId="u1"
      onToggleMyProjects={vi.fn()}
      onToggleAccess={vi.fn()}
      onTogglePerson={vi.fn()}
      onSelectCreated={vi.fn()}
    />
  );
}

describe("ProjectsFilterPanel", () => {
  it("badges panel picks but never My projects alone", () => {
    const first = render(
      <ProjectsFilterPanel
        filters={freshListFilters()}
        members={MEMBERS}
        selfId="u1"
        onToggleMyProjects={vi.fn()}
        onToggleAccess={vi.fn()}
        onTogglePerson={vi.fn()}
        onSelectCreated={vi.fn()}
      />
    );
    expect(screen.queryByTestId("projects-filter-badge")).toBeNull();
    first.unmount();
    badgeOnly({ ...freshListFilters(), myProjects: true });
    expect(screen.queryByTestId("projects-filter-badge")).toBeNull();
  });

  it("shows the badge once a panel value applies", () => {
    badgeOnly({ ...freshListFilters(), access: [0] });
    expect(screen.getByTestId("projects-filter-badge")).not.toBeNull();
  });

  it("searches values case-insensitively and clears", async () => {
    const { user } = await setup();

    await user.clear(screen.getByPlaceholderText("Search"));
    await user.type(screen.getByPlaceholderText("Search"), "RIV");
    expect(screen.getByText("Private")).not.toBeNull();
    expect(screen.queryByText("Public")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Clear filter search" }));
    expect(screen.getByText("Public")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Clear filter search" })).toBeNull();
  });

  it.each(["Priv", "priv", "PRIV"])("keeps the title-case access option for the query %s", async (query) => {
    const { user } = await setup();

    await user.type(screen.getByPlaceholderText("Search"), query);
    expect(screen.getByText("Private")).not.toBeNull();
    expect(screen.queryByText("Public")).toBeNull();
  });

  it("folds case when narrowing created-date presets", async () => {
    const { user } = await setup();

    await user.type(screen.getByPlaceholderText("Search"), "LaSt 7");
    expect(screen.getByText("Last 7 days")).not.toBeNull();
    expect(screen.queryByText("Last 30 days")).toBeNull();
    expect(screen.queryByText("Today")).toBeNull();
  });

  it("reports empty sections while searching", async () => {
    const { user } = await setup();

    await user.type(screen.getByPlaceholderText("Search"), "zzz");
    expect(screen.getAllByText("No matches found").length).toBeGreaterThan(0);
    expect(screen.queryByText("Private")).toBeNull();
    expect(screen.queryByText("Custom")).toBeNull();
  });

  it("toggles ownership, access and people", async () => {
    const { user, handlers } = await setup();

    await user.click(screen.getByText("My projects"));
    expect(handlers.onToggleMyProjects).toHaveBeenCalledTimes(1);

    await user.click(screen.getByText("Private"));
    expect(handlers.onToggleAccess).toHaveBeenCalledWith(0);

    const lead = screen.getByRole("region", { name: "Lead" });
    await user.click(within(lead).getByText("You"));
    expect(handlers.onTogglePerson).toHaveBeenCalledWith("lead", "u1");

    const members = screen.getByRole("region", { name: "Members" });
    await user.click(within(members).getByText("Alan Turing"));
    expect(handlers.onTogglePerson).toHaveBeenCalledWith("members", "u3");
  });

  it("names person checkboxes by label text alone, without the avatar", async () => {
    await setup();

    // The avatar is decorative inside the label: its img name must not
    // leak into the checkbox name (self reads exactly "You", not
    // "Ada Lovelace You").
    const lead = screen.getByRole("region", { name: "Lead" });
    expect(within(lead).getByRole("checkbox", { name: "You" })).not.toBeNull();
    expect(within(lead).queryByRole("checkbox", { name: "Ada Lovelace You" })).toBeNull();
    const members = screen.getByRole("region", { name: "Members" });
    expect(within(members).getByRole("checkbox", { name: "Alan Turing" })).not.toBeNull();
  });

  it("counts picks on collapsible headers", async () => {
    const { user } = await setup({
      filters: {
        ...freshListFilters(),
        access: [0],
        lead: ["u1"],
        createdAt: ["last_7_days;custom;custom"],
      },
    });

    expect(screen.getByRole("button", { name: "Access (1)" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "Created date (1)" })).not.toBeNull();

    const lead = screen.getByRole("region", { name: "Lead" });
    expect(within(lead).getByText("You")).not.toBeNull();
    await user.click(screen.getByRole("button", { name: /Lead/ }));
    expect(within(lead).queryByText("You")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Access/ }));
    expect(screen.queryByText("Private")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Created date/ }));
    expect(screen.queryByText("Last 7 days")).toBeNull();
  });

  it("shows a loading window while memberships resolve", async () => {
    await setup({ members: undefined });

    expect(screen.getAllByRole("status", { name: "Loading people" })).toHaveLength(2);
    expect(screen.queryByText("You")).toBeNull();
  });

  it("pages long people lists behind view all", async () => {
    const { user } = await setup();

    const lead = screen.getByRole("region", { name: "Lead" });
    expect(within(lead).queryByText("Grace Hopper")).toBeNull();
    await user.click(within(lead).getByRole("button", { name: "View all" }));
    expect(within(lead).getByText("Grace Hopper")).not.toBeNull();
    await user.click(within(lead).getByRole("button", { name: "View less" }));
    expect(within(lead).queryByText("Grace Hopper")).toBeNull();
  });

  it("single-selects presets and deselects the current one", async () => {
    const { user, handlers } = await setup();

    await user.click(screen.getByText("Last 7 days"));
    expect(handlers.onSelectCreated).toHaveBeenCalledWith(["last_7_days;custom;custom"]);
  });

  it("deselects the active preset on a second click", async () => {
    const { user, handlers } = await setup({
      filters: { ...freshListFilters(), createdAt: ["last_7_days;custom;custom"] },
    });

    await user.click(screen.getByText("Last 7 days"));
    expect(handlers.onSelectCreated).toHaveBeenCalledWith([]);
  });

  it("opens the custom dialog and applies its range", async () => {
    const { user, handlers } = await setup();

    await user.click(screen.getByText("Custom"));
    expect(screen.getByRole("dialog", { name: "Created date" })).not.toBeNull();
    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2026-10-01" } });
    fireEvent.change(screen.getByLabelText("End date"), { target: { value: "2026-10-31" } });
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(handlers.onSelectCreated).toHaveBeenCalledWith(["2026-10-01;after", "2026-10-31;before"]);
  });

  it("clears the custom range from its row", async () => {
    const { user, handlers } = await setup({
      filters: { ...freshListFilters(), createdAt: ["2026-10-01;after", "2026-10-31;before"] },
    });

    await user.click(screen.getByText("Custom"));
    expect(handlers.onSelectCreated).toHaveBeenCalledWith([]);
    expect(screen.queryByRole("dialog", { name: "Created date" })).toBeNull();
  });
});
