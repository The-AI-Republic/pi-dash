// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails, WorkspaceMember } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the card content, the click
// interception and the quick actions.
vi.mock("@tanstack/react-router", () => ({
  Link: (props: {
    params?: Record<string, string>;
    children?: React.ReactNode;
    className?: string;
    onClick?: (event: React.MouseEvent) => void;
    onContextMenu?: (event: React.MouseEvent) => void;
    ref?: React.Ref<HTMLAnchorElement>;
    ["data-testid"]?: string;
  }) => (
    <a
      href={`/acme/projects/${props.params?.["projectId"] ?? ""}/issues`}
      className={props.className}
      data-testid={props["data-testid"]}
      onClick={props.onClick}
      onContextMenu={props.onContextMenu}
      ref={props.ref}
    >
      {props.children}
    </a>
  ),
}));

vi.mock("../../../shared/shell/toaster.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { formatCreationDate, ProjectCard, type ProjectCardProps } from "./ProjectCard.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const MEMBER_A = "323e4567-e89b-12d3-a456-426614174002";
const MEMBER_B = "423e4567-e89b-12d3-a456-426614174003";
const MEMBER_C = "523e4567-e89b-12d3-a456-426614174004";
const MEMBER_D = "623e4567-e89b-12d3-a456-426614174005";

function project(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PROJECT_ID,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name: "Website relaunch",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function memberRow(id: string, displayName: string, avatarUrl: string | null = null): WorkspaceMember {
  return {
    id,
    member: {
      id,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: avatarUrl,
      is_bot: false,
      display_name: displayName,
    },
    role: 15,
  };
}

const CLIPBOARD_DESCRIPTOR = Object.getOwnPropertyDescriptor(globalThis.navigator, "clipboard");
const assign = vi.fn();

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(globalThis.navigator, "clipboard", { value: { writeText }, configurable: true });
}

function renderCard(props: Partial<ProjectCardProps> = {}, row: ProjectDetails | null = null): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ProjectCard workspaceSlug="acme" project={row ?? project()} {...props} />
    </QueryClientProvider>
  );
}

function stubJoinClient(joinStatus = 201) {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/projects/invitations/")) {
      return {
        status: joinStatus,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(joinStatus === 201 ? { message: "joined" } : { detail: "refused" }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
}

function openOverflow(): void {
  fireEvent.click(screen.getByTestId("project-card-actions"));
}

function menuNames(): string[] {
  return screen.getAllByRole("menuitem").map((item) => item.textContent ?? "");
}

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(globalThis.window, "location", {
    value: { assign, origin: "https://app.test" },
    writable: true,
    configurable: true,
  });
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  if (CLIPBOARD_DESCRIPTOR === undefined) {
    Reflect.deleteProperty(globalThis.navigator, "clipboard");
  } else {
    Object.defineProperty(globalThis.navigator, "clipboard", CLIPBOARD_DESCRIPTOR);
  }
});

describe("ProjectCard identity", () => {
  it("links the project name into its work-items view", () => {
    renderCard({}, project({ name: "Website relaunch" }));

    const link = screen.getByRole("link", { name: /Website relaunch/ });
    expect(link).toHaveAttribute("href", `/acme/projects/${PROJECT_ID}/issues`);
    expect(link).toHaveAttribute("data-testid", "project-card");
    expect(screen.getByRole("heading", { name: "Website relaunch" })).not.toBeNull();
  });

  it("shows the short code with a private mark on private projects", () => {
    renderCard({}, project({ identifier: "SCRT", network: 0 }));

    expect(screen.getByText("SCRT")).not.toBeNull();
    expect(screen.getByText("Private project")).not.toBeNull();
  });

  it("shows no private mark on public projects", () => {
    renderCard({}, project({ identifier: "PUB", network: 2 }));

    expect(screen.getByText("PUB")).not.toBeNull();
    expect(screen.queryByText("Private project")).toBeNull();
  });

  it("renders the description when one is set", () => {
    renderCard({}, project({ description: "  The team backlog. " }));

    expect(screen.getByTestId("project-card-subtext")).toHaveTextContent("The team backlog.");
  });

  it("falls back to a generated creation line on a blank description", () => {
    renderCard({}, project({ description: "   " }));

    expect(screen.getByTestId("project-card-subtext")).toHaveTextContent("Created on Jan 2, 2024");
  });

  it("renders the logo box", () => {
    renderCard({}, project({ logo_props: { in_use: "emoji", emoji: { value: "🚀" } } }));

    expect(screen.getByTestId("project-logo")).toHaveTextContent("🚀");
  });

  it("renders no cover imagery", () => {
    renderCard(
      {},
      project({ name: "Website relaunch", cover_image_url: "https://img.test/cover.png", cover_image: "cover.png" })
    );

    expect(screen.queryByRole("img", { name: "Website relaunch" })).toBeNull();
  });
});

describe("ProjectCard member stack", () => {
  it("names an empty team", () => {
    renderCard({}, project({ members: [] }));

    expect(screen.getByTestId("project-member-stack-empty")).toHaveTextContent("No members yet");
  });

  it("resolves member names through the directory", () => {
    renderCard({ members: [memberRow(MEMBER_A, "Ada Lovelace")] }, project({ members: [MEMBER_A] }));

    expect(screen.getByRole("img", { name: "Ada Lovelace" })).not.toBeNull();
  });

  it("passes member avatar images through", () => {
    renderCard(
      { members: [memberRow(MEMBER_A, "Ada Lovelace", "https://img.test/a.png")] },
      project({ members: [MEMBER_A] })
    );

    expect(screen.getByRole("img", { name: "Ada Lovelace" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://img.test/a.png"
    );
  });

  it("falls back gracefully for members missing from the directory", () => {
    renderCard({}, project({ members: [MEMBER_A] }));

    expect(screen.getByRole("img", { name: "Project member" })).not.toBeNull();
  });

  it("collapses four members into an overflow bubble", () => {
    renderCard(
      {
        members: [
          memberRow(MEMBER_A, "Ada"),
          memberRow(MEMBER_B, "Bob"),
          memberRow(MEMBER_C, "Cat"),
          memberRow(MEMBER_D, "Dan"),
        ],
      },
      project({ members: [MEMBER_A, MEMBER_B, MEMBER_C, MEMBER_D] })
    );

    expect(screen.getByTestId("project-member-stack-overflow")).toHaveTextContent("+2");
  });
});

describe("ProjectCard star", () => {
  it("hides the star from guests", () => {
    renderCard({ canStar: false, onToggleFavorite: () => undefined });

    expect(screen.queryByTestId("project-card-star")).toBeNull();
  });

  it("hides the star until its toggle is wired", () => {
    renderCard({ canStar: true });

    expect(screen.queryByTestId("project-card-star")).toBeNull();
  });

  it("hides the star on archived cards", () => {
    renderCard({ canStar: true, onToggleFavorite: () => undefined }, project({ archived_at: "2024-05-06T07:08:09Z" }));

    expect(screen.queryByTestId("project-card-star")).toBeNull();
  });

  it("toggles the favorite on click without navigating", () => {
    const onToggleFavorite = vi.fn();
    renderCard({ canStar: true, onToggleFavorite }, project({ name: "Website relaunch", member_role: null }));

    fireEvent.click(screen.getByTestId("project-card-star"));
    expect(onToggleFavorite).toHaveBeenCalledTimes(1);
    expect(onToggleFavorite).toHaveBeenCalledWith(expect.objectContaining({ id: PROJECT_ID }));
    expect(screen.queryByRole("heading", { name: "Join Project?" })).toBeNull();
  });

  it("names and fills the star by state", () => {
    const { rerender } = render(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectCard workspaceSlug="acme" project={project()} canStar={true} onToggleFavorite={() => undefined} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-card-star")).toHaveAttribute("aria-label", "Add Website relaunch to favorites");
    expect(screen.getByTestId("project-card-star").querySelector("svg")).toHaveAttribute("fill", "none");

    rerender(
      <QueryClientProvider client={new QueryClient()}>
        <ProjectCard
          workspaceSlug="acme"
          project={project({ is_favorite: true })}
          canStar={true}
          onToggleFavorite={() => undefined}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("project-card-star")).toHaveAttribute(
      "aria-label",
      "Remove Website relaunch from favorites"
    );
    expect(screen.getByTestId("project-card-star").querySelector("svg")).toHaveAttribute("fill", "currentColor");
  });

  it("pins the pending star while the write flies", () => {
    renderCard({ canStar: true, starPending: true, onToggleFavorite: () => undefined });

    const star = screen.getByTestId("project-card-star");
    expect(star).toBeDisabled();
    expect(star).toHaveAttribute("aria-busy", "true");
  });
});

describe("ProjectCard click routing", () => {
  it("lets member clicks navigate", () => {
    renderCard({}, project({ member_role: 15 }));

    const defaulted = fireEvent.click(screen.getByTestId("project-card"));
    expect(defaulted).toBe(true);
    expect(screen.queryByRole("heading", { name: "Join Project?" })).toBeNull();
  });

  it("intercepts outsider clicks toward joining", () => {
    renderCard({}, project({ member_role: null }));

    const defaulted = fireEvent.click(screen.getByTestId("project-card"));
    expect(defaulted).toBe(false);
    expect(screen.getByRole("heading", { name: "Join Project?" })).not.toBeNull();
  });

  it("never navigates archived cards", () => {
    renderCard({}, project({ member_role: 20, archived_at: "2024-05-06T07:08:09Z" }));

    const defaulted = fireEvent.click(screen.getByTestId("project-card"));
    expect(defaulted).toBe(false);
    expect(screen.queryByRole("heading", { name: "Join Project?" })).toBeNull();
  });
});

describe("ProjectCard menus", () => {
  it("offers settings and copy-link to project members", () => {
    renderCard({}, project({ member_role: 15 }));

    openOverflow();
    expect(menuNames()).toEqual(["Settings", "Copy link"]);
  });

  it("offers archive to project admins on active cards once wired", () => {
    renderCard({ onArchive: () => undefined }, project({ member_role: 20 }));

    openOverflow();
    expect(menuNames()).toEqual(["Settings", "Copy link", "Archive"]);
  });

  it("withholds archive from members without the admin standing", () => {
    renderCard({ onArchive: () => undefined }, project({ member_role: 15 }));

    openOverflow();
    expect(menuNames()).toEqual(["Settings", "Copy link"]);
  });

  it("withholds archive until its dialog is wired", () => {
    renderCard({}, project({ member_role: 20 }));

    openOverflow();
    expect(menuNames()).toEqual(["Settings", "Copy link"]);
  });

  it("offers join, open-in-tab and copy-link to outsiders", () => {
    renderCard({}, project({ member_role: null }));

    openOverflow();
    expect(menuNames()).toEqual(["Join", "Open in new tab", "Copy link"]);
  });

  it("offers copy-link alone to guest members", () => {
    renderCard({}, project({ member_role: 5 }));

    openOverflow();
    expect(menuNames()).toEqual(["Copy link"]);
  });

  it("offers restore and delete to project admins on archived cards", () => {
    renderCard(
      { onRestore: () => undefined, onDelete: () => undefined },
      project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 })
    );

    openOverflow();
    expect(menuNames()).toEqual(["Restore", "Delete"]);
  });

  it("hides the quick actions until restore and delete are wired", () => {
    renderCard({}, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 }));

    expect(screen.queryByTestId("project-card-actions")).toBeNull();
  });

  it("opens the same entries on right-click", () => {
    renderCard({}, project({ member_role: null }));

    fireEvent.contextMenu(screen.getByTestId("project-card"));
    expect(menuNames()).toEqual(["Join", "Open in new tab", "Copy link"]);
  });

  it("opens no menu where no entry applies", () => {
    renderCard({}, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 5 }));

    fireEvent.contextMenu(screen.getByTestId("project-card"));
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("opens the overflow menu without joining", () => {
    renderCard({}, project({ member_role: null }));

    openOverflow();
    expect(screen.getByRole("menu")).not.toBeNull();
    expect(screen.queryByRole("heading", { name: "Join Project?" })).toBeNull();
  });

  it("navigates to settings from the menu", () => {
    renderCard({}, project({ member_role: 15 }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Settings" }));
    expect(assign).toHaveBeenCalledWith(`/acme/settings/projects/${PROJECT_ID}`);
  });

  it("opens the join dialog from the menu", () => {
    renderCard({}, project({ member_role: null }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Join" }));
    expect(screen.getByRole("heading", { name: "Join Project?" })).not.toBeNull();
  });

  it("opens the issues view in a new tab from the menu", () => {
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    renderCard({}, project({ member_role: null }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in new tab" }));
    expect(open).toHaveBeenCalledWith(`https://app.test/acme/projects/${PROJECT_ID}/issues`, "_blank", "noopener");
  });

  it("copies the issues address from the menu", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    stubClipboard(writeText);
    renderCard({}, project({ member_role: 15 }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(`https://app.test/acme/projects/${PROJECT_ID}/issues`));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Copied the project link", expect.any(String)));
  });

  it("toasts when the clipboard is unavailable", async () => {
    Object.defineProperty(globalThis.navigator, "clipboard", { value: undefined, configurable: true });
    renderCard({}, project({ member_role: 15 }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't copy the link", expect.any(String)));
  });

  it("dispatches restore and delete to their dialogs", () => {
    const onRestore = vi.fn();
    const onDelete = vi.fn();
    renderCard({ onRestore, onDelete }, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));
    expect(onRestore).toHaveBeenCalledWith(expect.objectContaining({ id: PROJECT_ID }));
    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith(expect.objectContaining({ id: PROJECT_ID }));
  });

  it("dispatches archive to its dialog", () => {
    const onArchive = vi.fn();
    renderCard({ onArchive }, project({ member_role: 20 }));

    openOverflow();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(onArchive).toHaveBeenCalledWith(expect.objectContaining({ id: PROJECT_ID }));
  });

  it("keeps a menu choice from navigating through the card's own link", () => {
    // The menu renders in a portal, but React bubbles its events up the
    // component tree into the card anchor, so without a boundary the
    // click would navigate into the project and unmount the dialog the
    // entry just opened (ARCH-026).
    const onArchive = vi.fn();
    renderCard({ onArchive }, project({ member_role: 20 }));

    openOverflow();
    const entry = within(screen.getByRole("menu")).getByRole("menuitem", { name: "Archive" });
    const defaulted = fireEvent.click(entry);
    expect(defaulted).toBe(false);
    expect(onArchive).toHaveBeenCalledTimes(1);
  });
});

describe("ProjectCard footer", () => {
  it("links authorized members to settings", () => {
    renderCard({}, project({ member_role: 15 }));

    fireEvent.click(screen.getByRole("button", { name: "Website relaunch settings" }));
    expect(assign).toHaveBeenCalledWith(`/acme/settings/projects/${PROJECT_ID}`);
    expect(screen.queryByRole("heading", { name: "Join Project?" })).toBeNull();
  });

  it("marks guest members as joined", () => {
    renderCard({}, project({ member_role: 5 }));

    expect(screen.getByText("Joined")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Join" })).toBeNull();
  });

  it("offers outsiders an inline join", () => {
    renderCard({}, project({ member_role: null }));

    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    expect(screen.getByRole("heading", { name: "Join Project?" })).not.toBeNull();
  });

  it("mutes archived cards with the archived marker", () => {
    renderCard(
      { onRestore: () => undefined, onDelete: () => undefined },
      project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 })
    );

    expect(screen.getByText("Archived")).not.toBeNull();
    expect(screen.getByTestId("project-card").className).toContain("opacity-75");
    expect(screen.getByTestId("project-card").className).not.toContain("hover:border-(--accent)");
    expect(screen.queryByRole("button", { name: "Website relaunch settings" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Join" })).toBeNull();
  });

  it("keeps active cards interactive", () => {
    renderCard({}, project({ member_role: 15 }));

    expect(screen.queryByText("Archived")).toBeNull();
    expect(screen.getByTestId("project-card").className).toContain("hover:border-(--accent)");
    expect(screen.getByTestId("project-card").className).not.toContain("opacity-75");
  });

  it("offers admins inline restore and delete on archived cards", () => {
    const onRestore = vi.fn();
    const onDelete = vi.fn();
    renderCard({ onRestore, onDelete }, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 }));

    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(onRestore).toHaveBeenCalledWith(expect.objectContaining({ id: PROJECT_ID }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith(expect.objectContaining({ id: PROJECT_ID }));
    expect(assign).not.toHaveBeenCalled();
    expect(screen.queryByRole("heading", { name: "Join Project?" })).toBeNull();
  });

  it("shows each footer action only once its dialog is wired", () => {
    renderCard({ onRestore: () => undefined }, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 }));

    expect(screen.getByRole("button", { name: "Restore" })).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  });

  it("shows the delete footer action without restore", () => {
    renderCard({ onDelete: () => undefined }, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 }));

    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
    expect(screen.getByRole("button", { name: "Delete" })).not.toBeNull();
  });

  it("hides footer actions from non-admins and until wired", () => {
    renderCard(
      { onRestore: () => undefined, onDelete: () => undefined },
      project({ archived_at: "2024-05-06T07:08:09Z", member_role: 5 })
    );

    expect(screen.getByText("Archived")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  });

  it("hides footer actions on archived cards until wired", () => {
    renderCard({}, project({ archived_at: "2024-05-06T07:08:09Z", member_role: 20 }));

    expect(screen.getByText("Archived")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();
  });
});

describe("ProjectCard join landing", () => {
  it("lands inside the project through the default full-page load", async () => {
    stubJoinClient();
    renderCard({}, project({ member_role: null }));

    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    fireEvent.click(screen.getByRole("button", { name: "Join Project" }));
    await waitFor(() => expect(assign).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/issues`));
  });

  it("lands through the provided navigator when one is passed", async () => {
    const onNavigate = vi.fn();
    stubJoinClient();
    renderCard({ onNavigate }, project({ member_role: null }));

    fireEvent.click(screen.getByRole("button", { name: "Join" }));
    fireEvent.click(screen.getByRole("button", { name: "Join Project" }));
    await waitFor(() => expect(onNavigate).toHaveBeenCalledWith(`/acme/projects/${PROJECT_ID}/issues`));
    expect(assign).not.toHaveBeenCalled();
  });
});

describe("formatCreationDate", () => {
  it("renders a UTC calendar date", () => {
    expect(formatCreationDate("2024-01-02T03:04:05Z")).toBe("Jan 2, 2024");
  });
});
