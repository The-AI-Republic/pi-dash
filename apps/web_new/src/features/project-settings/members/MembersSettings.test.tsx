// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab body (PSET-018–021, PSET-027): the five columns with a
// profile-linked avatar, client-side search, per-project role filter and
// sort, the skeleton until both reads settle, the centered no-match note,
// and a failed read that toasts without clearing the table.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerMocks = vi.hoisted(() => ({ navigate: vi.fn(), pathname: "/acme/settings/projects/p/members/" }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerMocks.navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: routerMocks.pathname } }),
}));

const writeMocks = vi.hoisted(() => {
  const mutation = () => ({ mutate: vi.fn(), isPending: false, args: [] as unknown[] });
  return {
    add: mutation(),
    role: mutation(),
    remove: mutation(),
    leave: mutation(),
    defaults: mutation(),
    editor: { userId: "123e4567-e89b-12d3-a456-42661417400a", workspaceRole: 20, projectRole: 20, ready: true },
    project: { data: undefined, isPending: true, isSuccess: false, isError: false, error: null } as {
      data: unknown;
      isPending: boolean;
      isSuccess: boolean;
      isError: boolean;
      error: unknown;
    },
  };
});

vi.mock("./api.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api.js")>();
  return {
    ...actual,
    useProjectMemberships: vi.fn(),
    useWorkspaceDirectory: vi.fn(),
    notifyMembersLoadFailure: vi.fn(),
    useAddProjectMembers: () => writeMocks.add,
    useUpdateProjectMemberRole: (...args: unknown[]) => {
      writeMocks.role.args = args;
      return writeMocks.role;
    },
    useRemoveProjectMember: () => writeMocks.remove,
    useLeaveProject: () => writeMocks.leave,
    useSaveProjectDefaults: () => writeMocks.defaults,
    useMemberEditor: () => writeMocks.editor,
    useProjectDefaults: () => writeMocks.project,
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
const OUTSIDER = "123e4567-e89b-12d3-a456-42661417400e";

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
  extra: { email?: string | undefined; avatarUrl?: string | null; bot?: boolean; role?: number } = {}
): WorkspaceMemberList[number] {
  return {
    id: `${id}-membership`,
    role: extra.role ?? 15,
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
  routerMocks.navigate.mockClear();
  writeMocks.add.mutate.mockReset();
  writeMocks.role.mutate.mockReset();
  writeMocks.remove.mutate.mockReset();
  writeMocks.leave.mutate.mockReset();
  writeMocks.defaults.mutate.mockReset();
  writeMocks.editor.userId = ADA;
  writeMocks.editor.workspaceRole = 20;
  writeMocks.editor.projectRole = 20;
  writeMocks.editor.ready = true;
  writeMocks.project.data = undefined;
  writeMocks.project.isPending = true;
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

  it("labels the Account type menu with the order its role-word sort produces", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Sort by Account type" }));
    const ascending = await screen.findByRole("menuitem", { name: /A to Z/ });
    await user.click(ascending);
    // The role word is the sort key: A to Z reads Admin, Guest, Member,
    // which is exactly what the menu claims and what the table paints.
    expect(screen.getAllByTestId("member-roster-row").map((row) => row.textContent)).toEqual([
      expect.stringContaining("Ada Lovelace"),
      expect.stringContaining("Linus Torvalds"),
      expect.stringContaining("Grace Hopper"),
    ]);
    expect(screen.getByRole("button", { name: "Sort by Account type" })).toHaveAttribute(
      "data-sort-marker",
      "Ascending"
    );
    await user.click(screen.getByRole("button", { name: "Sort by Account type" }));
    await user.click(await screen.findByRole("menuitem", { name: /Z to A/ }));
    expect(screen.getAllByTestId("member-roster-row").map((row) => row.textContent)).toEqual([
      expect.stringContaining("Grace Hopper"),
      expect.stringContaining("Linus Torvalds"),
      expect.stringContaining("Ada Lovelace"),
    ]);
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

describe("MembersSettings writes (PSET-022–026)", () => {
  const PROJECT_ROW = {
    id: PROJECT,
    project_lead: null,
    default_assignee: null,
    guest_view_all_features: false,
  };

  it("shows the add control to admins only", () => {
    happyPath();
    renderSection();
    expect(screen.getByRole("button", { name: "Add member" })).toBeInTheDocument();
    cleanup();
    writeMocks.editor.workspaceRole = 15;
    writeMocks.editor.projectRole = 15;
    renderSection();
    expect(screen.queryByRole("button", { name: "Add member" })).not.toBeInTheDocument();
  });

  it("admits a workspace admin through the workspace role even on a member project role", () => {
    happyPath();
    writeMocks.editor.workspaceRole = 20;
    writeMocks.editor.projectRole = 15;
    renderSection();
    expect(screen.getByRole("button", { name: "Add member" })).toBeInTheDocument();
  });

  it("opens the invite dialog and bulk-adds its rows", async () => {
    happyPath();
    vi.mocked(useWorkspaceDirectory).mockReturnValue(
      resolved([
        ...DIRECTORY,
        directoryMember(OUTSIDER, "Phoebe", "Outsider", "Phoebe", { email: "p@example.test" }),
      ]) as never
    );
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Add member" }));
    expect(await screen.findByRole("heading", { name: "Invite members" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select co-worker" }));
    await user.click(await screen.findByRole("option", { name: /Phoebe Outsider/ }));
    await user.click(screen.getByRole("button", { name: "Add members" }));
    expect(writeMocks.add.mutate).toHaveBeenCalledWith(
      { members: [{ member_id: OUTSIDER, role: 15 }] },
      expect.objectContaining({ onSuccess: expect.any(Function) })
    );
  });

  it("offers a role editor on rows the rule admits and patches the membership id", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Change Grace Hopper's role" }));
    await user.click(await screen.findByRole("option", { name: "Admin" }));
    expect(writeMocks.role.mutate).toHaveBeenCalledWith({ memberId: "m-grace", role: 20 });
  });

  it("shows the picked role at once and puts the old word back when the write rolls back", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Change Grace Hopper's role" }));
    await user.click(await screen.findByRole("option", { name: "Admin" }));
    expect(within(graceRow).getByRole("button", { name: "Change Grace Hopper's role" })).toHaveTextContent("Admin");
    // A refusal reaches the hook's rollback callback with the row it touched.
    const rollback = writeMocks.role.args[2] as (result: { memberId: string }) => void;
    act(() => rollback({ memberId: "m-unknown" }));
    expect(within(graceRow).getByRole("button", { name: "Change Grace Hopper's role" })).toHaveTextContent("Admin");
    act(() => rollback({ memberId: "m-grace" }));
    expect(within(graceRow).getByRole("button", { name: "Change Grace Hopper's role" })).toHaveTextContent("Member");
  });

  it("offers every role on a project-guest row held by a workspace member", async () => {
    // Linus holds a project guest seat as a workspace member: the grant
    // must restore, so the editor offers Member and Admin too (PSET-023).
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const linusRow = screen.getAllByTestId("member-roster-row")[2]!;
    await user.click(within(linusRow).getByRole("button", { name: "Change Linus Torvalds's role" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual([
      "Guest",
      "Member",
      "Admin",
    ]);
  });

  it("caps a workspace guest's row at Guest even for an admin", async () => {
    vi.mocked(useProjectMemberships).mockReturnValue(resolved(MEMBERSHIPS) as never);
    vi.mocked(useWorkspaceDirectory).mockReturnValue(
      resolved([
        ...DIRECTORY.filter((entry) => entry.member.id !== LINUS),
        directoryMember(LINUS, "Linus", "Torvalds", "linus", { role: 5 }),
      ]) as never
    );
    const user = userEvent.setup();
    renderSection();
    const linusRow = screen.getAllByTestId("member-roster-row")[2]!;
    await user.click(within(linusRow).getByRole("button", { name: "Change Linus Torvalds's role" }));
    expect((await screen.findAllByRole("option")).map((option) => option.textContent)).toEqual(["Guest"]);
  });

  it("treats a member missing from the directory as having no workspace role", () => {
    happyPath();
    vi.mocked(useWorkspaceDirectory).mockReturnValue(
      resolved(DIRECTORY.filter((entry) => entry.member.id !== GRACE)) as never
    );
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    // An unknown workspace role offers nothing to an editor and blocks it.
    expect(within(graceRow).queryByRole("button", { name: "Change Grace Hopper's role" })).not.toBeInTheDocument();
    expect(within(graceRow).queryByRole("button", { name: "Actions for Grace Hopper" })).not.toBeInTheDocument();
  });

  it("leaves the role cell as text for a plain member", () => {
    happyPath();
    writeMocks.editor.workspaceRole = 15;
    writeMocks.editor.projectRole = 15;
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    expect(within(graceRow).queryByRole("button", { name: "Change Grace Hopper's role" })).not.toBeInTheDocument();
    expect(within(graceRow).getByText("Member")).toBeInTheDocument();
  });

  it("offers Leave on the caller's own row and routes to the project list", async () => {
    happyPath();
    const user = userEvent.setup();
    writeMocks.editor.userId = GRACE;
    writeMocks.editor.workspaceRole = 15;
    writeMocks.editor.projectRole = 15;
    writeMocks.leave.mutate.mockImplementation((_input: unknown, options: { onSuccess?: () => void }) => {
      options.onSuccess?.();
    });
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Leave" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Leave" }));
    expect(writeMocks.leave.mutate).toHaveBeenCalled();
    expect(routerMocks.navigate).toHaveBeenCalledWith({ to: "/$ws/projects", params: { ws: SLUG } });
  });

  it("removes another member through the row menu for an admin", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    expect(within(await screen.findByRole("dialog")).getByText("Remove Grace Hopper?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(writeMocks.remove.mutate).toHaveBeenCalledWith(
      { memberId: "m-grace" },
      expect.objectContaining({ onSettled: expect.any(Function) })
    );
  });

  it("offers no row menu to a plain member on someone else's row", () => {
    happyPath();
    writeMocks.editor.userId = LINUS;
    writeMocks.editor.workspaceRole = 15;
    writeMocks.editor.projectRole = 15;
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    expect(within(graceRow).queryByRole("button", { name: "Actions for Grace Hopper" })).not.toBeInTheDocument();
  });

  it("closes the invite dialog once the bulk add lands", async () => {
    happyPath();
    vi.mocked(useWorkspaceDirectory).mockReturnValue(
      resolved([
        ...DIRECTORY,
        directoryMember(OUTSIDER, "Phoebe", "Outsider", "Phoebe", { email: "p@example.test" }),
      ]) as never
    );
    const user = userEvent.setup();
    renderSection();
    await user.click(screen.getByRole("button", { name: "Add member" }));
    await user.click(screen.getByRole("button", { name: "Select co-worker" }));
    await user.click(await screen.findByRole("option", { name: /Phoebe Outsider/ }));
    await user.click(screen.getByRole("button", { name: "Add members" }));
    const options = writeMocks.add.mutate.mock.calls[0]?.[1] as { onSuccess?: () => void };
    act(() => options.onSuccess?.());
    expect(screen.queryByRole("heading", { name: "Invite members" })).not.toBeInTheDocument();
  });

  it("withdraws the remove confirmation once the write settles", async () => {
    happyPath();
    const user = userEvent.setup();
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Remove" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    const options = writeMocks.remove.mutate.mock.calls[0]?.[1] as { onSettled?: () => void };
    act(() => options.onSettled?.());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("withdraws the leave confirmation once the write settles without a refusal", async () => {
    happyPath();
    const user = userEvent.setup();
    writeMocks.editor.userId = GRACE;
    writeMocks.editor.workspaceRole = 15;
    writeMocks.editor.projectRole = 15;
    renderSection();
    const graceRow = screen.getAllByTestId("member-roster-row")[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Leave" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Leave" }));
    const options = writeMocks.leave.mutate.mock.calls[0]?.[1] as { onSettled?: () => void };
    act(() => options.onSettled?.());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(routerMocks.navigate).not.toHaveBeenCalled();
  });

  it("saves defaults for admins and disables them for members", async () => {
    happyPath();
    const user = userEvent.setup();
    writeMocks.project.data = PROJECT_ROW;
    writeMocks.project.isPending = false;
    renderSection();
    await user.click(screen.getByRole("button", { name: "Project lead" }));
    await user.click(await screen.findByRole("option", { name: "Grace H" }));
    expect(writeMocks.defaults.mutate).toHaveBeenCalledWith({ project_lead: GRACE });
    cleanup();
    writeMocks.editor.workspaceRole = 15;
    writeMocks.editor.projectRole = 15;
    renderSection();
    expect(screen.getByRole("button", { name: "Project lead" })).toBeDisabled();
  });
});
