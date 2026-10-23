// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Project, ProjectList } from "@pidash/api-client";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router Link (CrossAppLink.test.tsx precedent): card
// actions targeting migrated prefixes render as router links, whose
// navigation is TanStack's own tested behavior.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
      const { to, children, ...rest } = props;
      return (
        <a data-router-link="" data-to={to} href={to} {...(rest as React.AnchorHTMLAttributes<HTMLAnchorElement>)}>
          {children as React.ReactNode}
        </a>
      );
    },
  };
});

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/index.js";
import { sessionKeys, setSessionClient } from "../../../core/session/queries.js";
import { QuickstartGuide } from "./QuickstartGuide.js";

// Migrated-route links (F-09, NEWFRONT-528) render through the workspace
// routes this component ships in. The suite has no router; the router
// link's navigation is TanStack's own tested behavior, so it stands in as
// an anchor and the assertions still read the href.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
    const { to, children, ...rest } = props;
    return (
      <a href={to} {...rest}>
        {children}
      </a>
    );
  },
}));

const UUID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow(overrides: Partial<Project> = {}): Project {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    name: "Web",
    identifier: "WEB",
    sort_order: 1,
    logo_props: {},
    member_role: null,
    intake_count: 0,
    archived_at: null,
    workspace: UUID,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: true,
    inbox_view: false,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function workspaceRow(role: number, totalMembers: number): Record<string, unknown> {
  return {
    id: UUID,
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: totalMembers,
    role,
  };
}

function renderGuide(options: {
  role?: number;
  totalMembers?: number;
  projects?: ProjectList | undefined;
  seedWorkspace?: boolean;
  onCreateProject?: () => void;
}): { unmount: () => void } {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedWorkspace !== false) {
    queryClient.setQueryData(sessionKeys.workspaces, [
      workspaceRow(options.role ?? ROLE_ADMIN, options.totalMembers ?? 1),
    ]);
  }
  const rendered = render(
    <QueryClientProvider client={queryClient}>
      {/* jsdom cannot navigate: hold the default anchor behavior so link
          clicks exercise the visit recording without navigation noise. */}
      <div
        onClickCapture={(event) => {
          if ((event.target as HTMLElement).closest("a") !== null) event.preventDefault();
        }}
      >
        <QuickstartGuide
          workspaceSlug="acme"
          projects={"projects" in options ? options.projects : []}
          {...(options.onCreateProject === undefined ? {} : { onCreateProject: options.onCreateProject })}
        />
      </div>
    </QueryClientProvider>
  );
  return { unmount: rendered.unmount };
}

function cardByTitle(title: string): HTMLElement {
  const heading = screen.getByRole("heading", { name: title });
  const card = heading.closest("[data-card]");
  if (card === null) throw new Error(`card for ${title} is missing`);
  return card as HTMLElement;
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("QuickstartGuide", () => {
  it("shows the four cards with their actions on a fresh workspace", () => {
    renderGuide({});
    expect(screen.getByRole("region", { name: "Quickstart checklist" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create a project" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Invite your team" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Set up your workspace" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Make Pi Dash yours" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Get started" })).toHaveAttribute("href", "/acme/projects");
    expect(screen.getByRole("link", { name: "Get them in" })).toHaveAttribute("href", "/acme/settings/members");
    expect(screen.getByRole("link", { name: "Configure this workspace" })).toHaveAttribute("href", "/acme/settings");
    expect(screen.getByRole("link", { name: "Personalize now" })).toHaveAttribute("href", "/settings/profile/general");
  });

  it("marks the project card done once joined while the panel stays", () => {
    renderGuide({ projects: [projectRow({ member_role: ROLE_MEMBER })] });
    const card = cardByTitle("Create a project");
    expect(card).toHaveAttribute("data-done", "true");
    expect(within(card).getByText("Done")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Get started" })).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Quickstart checklist" })).toBeInTheDocument();
  });

  it("marks the team card done at two members", () => {
    renderGuide({ totalMembers: 2 });
    const card = cardByTitle("Invite your team");
    expect(card).toHaveAttribute("data-done", "true");
    expect(screen.queryByRole("link", { name: "Get them in" })).not.toBeInTheDocument();
  });

  it("completes the workspace card on a settings visit", async () => {
    const user = userEvent.setup();
    renderGuide({});
    expect(cardByTitle("Set up your workspace")).toHaveAttribute("data-done", "false");
    await user.click(screen.getByRole("link", { name: "Configure this workspace" }));
    expect(cardByTitle("Set up your workspace")).toHaveAttribute("data-done", "true");
    expect(window.localStorage.getItem("web_new.home.quickstart.acme")).toContain('"visited_workspace":true');
  });

  it("completes the profile card on a profile visit", async () => {
    const user = userEvent.setup();
    renderGuide({});
    await user.click(screen.getByRole("link", { name: "Personalize now" }));
    expect(cardByTitle("Make Pi Dash yours")).toHaveAttribute("data-done", "true");
    expect(window.localStorage.getItem("web_new.home.quickstart.acme")).toContain('"visited_profile":true');
  });

  it("records nothing when the invite action is followed", async () => {
    const user = userEvent.setup();
    renderGuide({});
    await user.click(screen.getByRole("link", { name: "Get them in" }));
    expect(window.localStorage.getItem("web_new.home.quickstart.acme")).toBeNull();
    expect(cardByTitle("Invite your team")).toHaveAttribute("data-done", "false");
  });

  it("dismisses the panel and stays hidden across renders", async () => {
    const user = userEvent.setup();
    const { unmount } = renderGuide({});
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("region", { name: "Quickstart checklist" })).not.toBeInTheDocument();
    expect(window.localStorage.getItem("web_new.home.quickstart.acme")).toContain('"hide":true');
    unmount();
    renderGuide({});
    expect(screen.queryByRole("region", { name: "Quickstart checklist" })).not.toBeInTheDocument();
  });

  it("auto-hides once the workspace is lived-in", () => {
    renderGuide({ totalMembers: 3, projects: [projectRow({ member_role: ROLE_MEMBER })] });
    expect(screen.queryByRole("region", { name: "Quickstart checklist" })).not.toBeInTheDocument();
  });

  it("leaves guests the profile action only", () => {
    renderGuide({ role: ROLE_GUEST });
    expect(screen.getByRole("region", { name: "Quickstart checklist" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Get started" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Get started" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Get them in" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Configure this workspace" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Personalize now" })).toBeInTheDocument();
  });

  it("withholds invite and setup from non-admins", () => {
    renderGuide({ role: ROLE_MEMBER });
    expect(screen.getByRole("link", { name: "Get started" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Get them in" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Configure this workspace" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Personalize now" })).toBeInTheDocument();
  });

  it("opens the create dialog through the opener when provided", async () => {
    const user = userEvent.setup();
    const onCreateProject = vi.fn();
    renderGuide({ onCreateProject });
    const action = screen.getByRole("button", { name: "Get started" });
    await user.click(action);
    expect(onCreateProject).toHaveBeenCalledTimes(1);
  });

  it("renders nothing while projects or the workspace load", () => {
    renderGuide({ projects: undefined, seedWorkspace: true });
    expect(screen.queryByRole("region", { name: "Quickstart checklist" })).not.toBeInTheDocument();
  });

  it("renders nothing while the workspace is missing", () => {
    renderGuide({ projects: [], seedWorkspace: false });
    expect(screen.queryByRole("region", { name: "Quickstart checklist" })).not.toBeInTheDocument();
  });
});
