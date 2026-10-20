// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab body (PSET-018–021, PSET-027): the five columns with a
// profile-linked avatar, client-side search, per-project role filter and
// sort, the skeleton until both reads settle, the centered no-match note,
// and a failed read that toasts without clearing the table.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api.js")>();
  return {
    ...actual,
    useProjectMemberships: vi.fn(),
    useWorkspaceDirectory: vi.fn(),
    notifyMembersLoadFailure: vi.fn(),
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyMembersLoadFailure, useProjectMemberships, useWorkspaceDirectory } from "./api.js";
import { clearMemberFilters } from "./filters.js";
import { MembersSettings, MEMBERS_NO_MATCH_TEXT } from "./MembersSettings.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "223e4567-e89b-12d3-a456-426614174002";

const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";
const LINUS = "123e4567-e89b-12d3-a456-42661417400c";
const BOT = "123e4567-e89b-12d3-a456-42661417400d";

const MEMBERSHIPS: ProjectMemberList = [
  { id: "m-ada", role: 20, member: ADA, project: PROJECT, original_role: 20, created_at: "2025-01-10T09:00:00Z" },
  { id: "m-grace", role: 15, member: GRACE, project: PROJECT, original_role: 15, created_at: "2025-03-02T09:00:00Z" },
  { id: "m-linus", role: 5, member: LINUS, project: PROJECT, original_role: 5, created_at: "2025-02-01T09:00:00Z" },
  { id: "m-bot", role: 15, member: BOT, project: PROJECT, original_role: 15, created_at: "2025-01-01T09:00:00Z" },
];

function directoryMember(
  id: string,
  first: string,
  last: string,
  display: string,
  extra: { email?: string | undefined; avatarUrl?: string | null; bot?: boolean } = {}
): WorkspaceMemberList[number] {
  return {
    id: `${id}-membership`,
    role: 15,
    member: {
      id,
      first_name: first,
      last_name: last,
      avatar: "",
      avatar_url: extra.avatarUrl ?? null,
      display_name: display,
      is_bot: extra.bot ?? false,
      ...(extra.email === undefined ? {} : { email: extra.email }),
    },
  };
}

const DIRECTORY: WorkspaceMemberList = [
  directoryMember(ADA, "Ada", "Lovelace", "ada", { email: "ada@example.test", avatarUrl: "/media/ada.png" }),
  directoryMember(GRACE, "Grace", "Hopper", "Grace H", { email: "grace@example.test" }),
  directoryMember(LINUS, "Linus", "Torvalds", "linus"),
  directoryMember(BOT, "Build", "Bot", "build-bot", { bot: true, email: "bot@example.test" }),
];

/** A settled query result for the mocked read hooks. */
function resolved<T>(data: T) {
  return { data, isPending: false, isSuccess: true, isError: false, error: null };
}

function pending() {
  return { data: undefined, isPending: true, isSuccess: false, isError: false, error: null };
}

function failed(error: unknown) {
  return { data: undefined, isPending: false, isSuccess: false, isError: true, error };
}

function renderSection(projectId: string = PROJECT): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <MembersSettings workspaceSlug={SLUG} projectId={projectId} resolveAvatar={(path) => path} />
    </QueryClientProvider>
  );
}

function rowEmails(): string[] {
  return screen.getAllByTestId("member-roster-row").map((row) => row.getAttribute("data-user-id") ?? "");
}

function happyPath(): void {
  vi.mocked(useProjectMemberships).mockReturnValue(resolved(MEMBERSHIPS) as never);
  vi.mocked(useWorkspaceDirectory).mockReturnValue(resolved(DIRECTORY) as never);
}

beforeEach(() => {
  clearMemberFilters();
  vi.mocked(notifyMembersLoadFailure).mockClear();
  vi.mocked(useProjectMemberships).mockReset();
  vi.mocked(useWorkspaceDirectory).mockReset();
});

afterEach(() => {
  clearMemberFilters();
  resetAppClient();
});

describe("MembersSettings table", () => {
  it("shows the five columns, non-bot rows and a profile-linked avatar", () => {
    happyPath();
    renderSection();
    const table = screen.getByRole("table", { name: "Members" });
    for (const column of ["Full name", "Display name", "Email", "Account type", "Joining date"]) {
      expect(within(table).getByRole("button", { name: `Sort by ${column}` })).toBeInTheDocument();
    }
    expect(rowEmails()).toEqual([ADA, GRACE, LINUS]);
    const adaRow = screen.getAllByTestId("member-roster-row")[0]!;
    expect(within(adaRow).getByText("Ada Lovelace")).toBeInTheDocument();
    expect(within(adaRow).getByText("ada")).toBeInTheDocument();
    expect(within(adaRow).getByText("ada@example.test")).toBeInTheDocument();
    expect(within(adaRow).getByText("Admin")).toBeInTheDocument();
    expect(within(adaRow).getByText("Jan 10, 2025")).toBeInTheDocument();
    expect(within(adaRow).getByRole("link", { name: "Open Ada Lovelace's profile" })).toHaveAttribute(
      "href",
      `/acme/profile/${ADA}`
    );
    expect(within(adaRow).getByRole("img", { name: "Ada Lovelace" }).querySelector("img")).toHaveAttribute(
      "src",
      "/media/ada.png"
    );
  });

  it("renders a blank email when the directory omits it and an initial avatar without a photo", () => {
    happyPath();
    renderSection();
    const linusRow = screen.getAllByTestId("member-roster-row")[2]!;
    expect(within(linusRow).getByText("Guest")).toBeInTheDocument();
    expect(within(linusRow).queryByText("@")).not.toBeInTheDocument();
  });

  it("leaves the joining cell empty when the membership has no date", () => {
    vi.mocked(useProjectMemberships).mockReturnValue(
      resolved(MEMBERSHIPS.map((row) => (row.member === LINUS ? { ...row, created_at: "" } : row))) as never
    );
    vi.mocked(useWorkspaceDirectory).mockReturnValue(resolved(DIRECTORY) as never);
    renderSection();
    const linusRow = screen.getAllByTestId("member-roster-row")[2]!;
    expect(within(linusRow).getByText("Guest")).toBeInTheDocument();
    expect(linusRow.querySelector("time")).toBeNull();
  });

  it("resolves avatars against the app client's origin by default", () => {
    happyPath();
    createAppClient("https://api.test");
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} />
      </QueryClientProvider>
    );
    const adaRow = screen.getAllByTestId("member-roster-row")[0]!;
    expect(within(adaRow).getByRole("img", { name: "Ada Lovelace" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://api.test/media/ada.png"
    );
  });
});

describe("MembersSettings search", () => {
  it("narrows by display and full name case-insensitively, then restores", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const search = screen.getByRole("textbox", { name: "Search members" });
    await user.type(search, "LOVE");
    expect(rowEmails()).toEqual([ADA]);
    await user.clear(search);
    await user.type(search, "grace h");
    expect(rowEmails()).toEqual([GRACE]);
    await user.clear(search);
    expect(rowEmails()).toEqual([ADA, GRACE, LINUS]);
  });

  it("shows the centered no-match notice for zero matches and hides it again", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const search = screen.getByRole("textbox", { name: "Search members" });
    await user.type(search, "nobody-here");
    expect(screen.getByText(MEMBERS_NO_MATCH_TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Members" })).not.toBeInTheDocument();
    await user.clear(search);
    expect(screen.queryByText(MEMBERS_NO_MATCH_TEXT)).not.toBeInTheDocument();
    expect(screen.getByRole("table", { name: "Members" })).toBeInTheDocument();
  });

  it("matches nothing when the role filter excludes every row", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Filters" }));
    await user.click(await screen.findByRole("checkbox", { name: "Admin" }));
    await user.click(screen.getByRole("checkbox", { name: "Guest" }));
    // Admin and Guest checked still leaves Grace out; the table shows two.
    expect(rowEmails()).toEqual([ADA, LINUS]);
    fireEvent.change(screen.getByRole("textbox", { name: "Search members" }), { target: { value: "grace" } });
    expect(screen.getByText(MEMBERS_NO_MATCH_TEXT)).toBeInTheDocument();
  });
});

describe("MembersSettings role filter", () => {
  it("offers one checkbox per role and narrows the table", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Filters" }));
    const options = [
      await screen.findByRole("checkbox", { name: "Admin" }),
      screen.getByRole("checkbox", { name: "Member" }),
      screen.getByRole("checkbox", { name: "Guest" }),
    ];
    expect(options).toHaveLength(3);
    await user.click(options[1]!);
    expect(rowEmails()).toEqual([GRACE]);
    expect(screen.getByTestId("member-filter-active")).toBeInTheDocument();
    await user.click(options[1]!);
    expect(rowEmails()).toEqual([ADA, GRACE, LINUS]);
    expect(screen.queryByTestId("member-filter-active")).not.toBeInTheDocument();
  });

  it("keeps the choice across a remount for the same project only", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Filters" }));
    await user.click(await screen.findByRole("checkbox", { name: "Member" }));
    expect(rowEmails()).toEqual([GRACE]);
    cleanup();
    renderSection();
    expect(rowEmails()).toEqual([GRACE]);
    expect(screen.getByTestId("member-filter-active")).toBeInTheDocument();
    cleanup();
    renderSection(OTHER_PROJECT);
    expect(rowEmails()).toEqual([ADA, GRACE, LINUS]);
    expect(screen.queryByTestId("member-filter-active")).not.toBeInTheDocument();
  });
});

describe("MembersSettings sort", () => {
  it("carries the direction marker on the sort trigger itself, not on a child", () => {
    happyPath();
    renderSection();
    const trigger = screen.getByRole("button", { name: "Sort by Display name" });
    expect(trigger).toHaveAttribute("data-sort-marker", "Not sorted");
    expect(trigger.querySelector("[data-sort-marker]")).toBeNull();
  });

  it("orders ascending, descending and clears, marking only the active column", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await user.click(await screen.findByRole("menuitem", { name: /A to Z/ }));
    expect(screen.getAllByTestId("member-roster-row").map((row) => row.textContent)).toEqual([
      expect.stringContaining("ada"),
      expect.stringContaining("Grace H"),
      expect.stringContaining("linus"),
    ]);
    expect(screen.getByRole("button", { name: "Sort by Display name" })).toHaveAttribute(
      "aria-label",
      "Sort by Display name"
    );
    // The driver reads the marker off the trigger button, so the active
    // column must paint it there; the other columns stay "Not sorted".
    expect(screen.getByRole("button", { name: "Sort by Display name" })).toHaveAttribute(
      "data-sort-marker",
      "Ascending"
    );
    expect(screen.getByRole("button", { name: "Sort by Email" })).toHaveAttribute("data-sort-marker", "Not sorted");
    await user.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await user.click(await screen.findByRole("menuitem", { name: /Z to A/ }));
    expect(screen.getAllByTestId("member-roster-row")[0]?.textContent).toContain("linus");
    expect(screen.getByRole("button", { name: "Sort by Display name" })).toHaveAttribute(
      "data-sort-marker",
      "Descending"
    );
    await user.click(screen.getByRole("button", { name: "Sort by Display name" }));
    await user.click(await screen.findByRole("menuitem", { name: "Clear sorting" }));
    expect(rowEmails()).toEqual([ADA, GRACE, LINUS]);
    expect(screen.getByRole("button", { name: "Sort by Display name" })).toHaveAttribute(
      "data-sort-marker",
      "Not sorted"
    );
  });

  it("keeps the sort across a remount", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Sort by Full name" }));
    await user.click(await screen.findByRole("menuitem", { name: /Z to A/ }));
    expect(rowEmails()).toEqual([LINUS, GRACE, ADA]);
    cleanup();
    renderSection();
    expect(rowEmails()).toEqual([LINUS, GRACE, ADA]);
  });
});

describe("MembersSettings states", () => {
  it("shows the skeleton until both reads settle", () => {
    vi.mocked(useProjectMemberships).mockReturnValue(pending() as never);
    vi.mocked(useWorkspaceDirectory).mockReturnValue(resolved(DIRECTORY) as never);
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient()}>
        <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={(path) => path} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("members-skeleton")).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Members" })).not.toBeInTheDocument();
    happyPath();
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={(path) => path} />
      </QueryClientProvider>
    );
    expect(screen.queryByTestId("members-skeleton")).not.toBeInTheDocument();
  });

  it("shows the skeleton while only the directory is pending", () => {
    vi.mocked(useProjectMemberships).mockReturnValue(resolved(MEMBERSHIPS) as never);
    vi.mocked(useWorkspaceDirectory).mockReturnValue(pending() as never);
    renderSection();
    expect(screen.getByTestId("members-skeleton")).toBeInTheDocument();
  });

  it("toasts a failed directory read once and keeps the table", async () => {
    const error = new Error("server said no");
    vi.mocked(useProjectMemberships).mockReturnValue(resolved(MEMBERSHIPS) as never);
    vi.mocked(useWorkspaceDirectory).mockReturnValue(failed(error) as never);
    renderSection();
    await waitFor(() => expect(notifyMembersLoadFailure).toHaveBeenCalledWith(error));
    expect(notifyMembersLoadFailure).toHaveBeenCalledTimes(1);
  });

  it("toasts a failed membership read too", async () => {
    const first = new Error("first");
    const second = new Error("second");
    vi.mocked(useProjectMemberships).mockReturnValue(failed(first) as never);
    vi.mocked(useWorkspaceDirectory).mockReturnValue(failed(second) as never);
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient()}>
        <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={(path) => path} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(notifyMembersLoadFailure).toHaveBeenCalledTimes(2));
    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={(path) => path} />
      </QueryClientProvider>
    );
    expect(notifyMembersLoadFailure).toHaveBeenCalledTimes(2);
  });
});
