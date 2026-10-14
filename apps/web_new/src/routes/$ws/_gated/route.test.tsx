// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { isRedirect } from "@tanstack/react-router";
import type * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  clearCommands,
  clearPalettePages,
  findPalettePage,
  listCommands,
  type PalettePage,
  type PidashCommand,
  type SearchResultHit,
} from "../../../shared/commands/index.js";

const harness = vi.hoisted(() => ({
  commands: [] as PidashCommand[],
  landing: "/",
  signOutError: null as Error | null,
  settled: false,
  searchScope: null as { workspaceSlug: string; projectId: string | null } | null,
  searchBoxScope: null as { workspaceSlug: string; projectId: string | null } | null,
  onSelectSearchResult: null as ((hit: SearchResultHit) => void) | null,
  role: 15,
  creationDisabled: false,
  projectViews: { cycle_view: true, module_view: true, issue_views_view: true, page_view: true, inbox_view: true },
  fetched: [] as string[],
  navigated: [] as unknown[],
  topBar: null as null | {
    workspaceSlug: string;
    section: string;
    currentPath: string;
    sidebarCollapsed: boolean;
    sidebarToggleEnabled: boolean;
    onToggleSidebar: () => void;
    unreadTotal: number;
    unreadMentions: number;
  },
  sidebarCollapsed: false,
  rail: undefined as unknown,
  routerState: {
    matches: [{ params: {} }],
    location: { pathname: "/acme/projects/p1/issues" },
  } as { matches: Array<{ params: Record<string, unknown> }>; location: { pathname: string } },
}));

// Shell-query stand-in: this suite owns the layout wiring (sign-out entry
// points through the shared action, top chrome props), not data fetching.
vi.mock("@tanstack/react-query", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-query")>();
  return { ...original, useSuspenseQuery: vi.fn(), useQuery: vi.fn() };
});

// Router stand-ins: the outlet plus the selectors the layout reads
// (matches for the active project, location for rail active states);
// route creation, redirects and the Route hooks stay real. The
// global navigate stand-in serves lazily mounted dialogs (the layout
// itself navigates through Route.useNavigate, spied per test).
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    useRouterState: (args: { select: (state: unknown) => unknown }) => args.select(harness.routerState),
    Outlet: () => null,
    useNavigate: () => (target: unknown) => harness.navigated.push(target),
  };
});

// The sign-out mutation resolves to a controllable landing (or throws);
// the action and command descriptors under test stay real.
vi.mock("../../../features/auth/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../features/auth/index.js")>();
  return {
    ...original,
    useSignOut: () => ({
      mutateAsync: async () => {
        try {
          if (harness.signOutError !== null) throw harness.signOutError;
          return harness.landing;
        } finally {
          harness.settled = true;
        }
      },
    }),
  };
});

// The deactivation dialog the layout mounts imports its mutation straight
// from the auth api module; stub it (the dialog's own suite owns its
// behavior).
vi.mock("../../../features/auth/api.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../features/auth/api.js")>();
  return {
    ...original,
    useDeactivateAccount: () => ({ mutateAsync: async () => "/?deactivated=1", isPending: false }),
  };
});

// Shell-chrome stand-ins: capture the registered commands, the palette
// search wiring, the top-bar wiring and the rail node, and expose the
// sidebar sign-out entry as a button; the chrome components own their own
// suites. Stores stay real.
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  const React = await import("react");
  return {
    ...original,
    AppShell: (props: {
      commands?: PidashCommand[];
      topBar?: React.ReactNode;
      sidebar?: React.ReactNode;
      rail?: React.ReactNode;
      children?: React.ReactNode;
      searchScope: { workspaceSlug: string; projectId: string | null };
      onSelectSearchResult: (hit: SearchResultHit) => void;
    }) => {
      harness.commands = [...(props.commands ?? [])];
      harness.searchScope = props.searchScope;
      harness.onSelectSearchResult = props.onSelectSearchResult;
      harness.rail = props.rail;
      return React.createElement("div", null, props.topBar, props.sidebar, props.children);
    },
    Sidebar: (props: {
      userLabel: string;
      onSignOut: () => void;
      onDeactivateAccount: () => void;
      collapsed?: boolean;
    }) => {
      harness.sidebarCollapsed = props.collapsed ?? false;
      return React.createElement(
        React.Fragment,
        null,
        React.createElement(
          "button",
          { type: "button", "aria-label": `Sign out ${props.userLabel}`, onClick: props.onSignOut },
          "Sign out"
        ),
        React.createElement("button", { type: "button", onClick: props.onDeactivateAccount }, "Deactivate account")
      );
    },
    TopBarSearch: (props: { scope: { workspaceSlug: string; projectId: string | null } | null }) => {
      harness.searchBoxScope = props.scope;
      return null;
    },
    TopBar: (props: NonNullable<typeof harness.topBar> & { searchBox?: React.ReactNode }) => {
      harness.topBar = { ...props };
      return React.createElement("div", { "data-testid": "topbar-standin" }, props.searchBox);
    },
  };
});

import { createAppClient } from "../../../core/api/index.js";
import { edition } from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import { useRailState, useShellChrome } from "../../../shared/shell/index.js";
import { Route } from "./route.js";

function PageProbe({ page, query }: { page: PalettePage; query: string }) {
  const state = page.usePageState(query);
  if (state.status === "loading") return <p>loading</p>;
  return (
    <ul>
      {state.options.map((option) => (
        <li key={option.id}>
          <button type="button" onClick={() => void option.run()}>
            {option.title}
          </button>
        </li>
      ))}
    </ul>
  );
}

describe("gated layout guard", () => {
  const me = { is_password_autoset: false };

  function stubContext(values: Record<string, unknown>) {
    return {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            const value = values[options.queryKey[1] as string];
            if (value instanceof Error) throw value;
            if (value === undefined) throw new Error("no fixture");
            return value;
          },
        },
      },
      search: {},
      location: { href: "/acme/projects/p/issues" },
      params: {},
    } as never;
  }

  it("bounces signed-out visitors to the entry with a return path", async () => {
    const failed = new Error("signed out");
    const args = stubContext({ me: failed });
    try {
      await Route.options.beforeLoad?.(args);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      const options = (error as unknown as { options: { to: unknown; search: unknown } }).options;
      expect(options.to).toBe("/");
      expect(options.search).toEqual({ next_path: "/acme/projects/p/issues" });
      return;
    }
    expect.unreachable("expected a redirect");
  });

  it("funnels unfinished members to onboarding", async () => {
    const args = stubContext({ me, profile: { is_onboarded: false } });
    try {
      await Route.options.beforeLoad?.(args);
    } catch (error) {
      expect(isRedirect(error)).toBe(true);
      expect((error as unknown as { options: { href: string } }).options.href).toBe("/onboarding");
      return;
    }
    expect.unreachable("expected a redirect");
  });

  it("renders for onboarded members", async () => {
    await expect(Route.options.beforeLoad?.(stubContext({ me, profile: { is_onboarded: true } }))).resolves.toEqual({});
  });

  it("renders for members whose flags are all set even without the marker", async () => {
    const profile = {
      is_onboarded: false,
      onboarding_step: {
        profile_complete: true,
        workspace_create: true,
        workspace_invite: true,
        workspace_join: true,
      },
    };
    await expect(Route.options.beforeLoad?.(stubContext({ me, profile }))).resolves.toEqual({});
  });
});

describe("gated layout loader", () => {
  it("prefetches workspaces, projects and the unread count", async () => {
    const seen: Array<readonly unknown[]> = [];
    const args = {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            seen.push(options.queryKey);
            return [];
          },
        },
      },
      params: { ws: "acme" },
    } as never;
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await loader(args);
    expect(seen).toEqual([
      ["session", "workspaces"],
      ["ws", "acme", "projects", "list"],
      ["ws", "acme", "notifications", "unread"],
    ]);
  });
});

describe("gated layout shell wiring", () => {
  const navigate = vi.fn();
  let mounted: { unmount: () => void } | null = null;

  function stubShellQueries(unread: unknown = undefined): void {
    const suspenseQuery = vi.mocked(useSuspenseQuery) as unknown as {
      mockImplementation: (run: (options: { queryKey: readonly unknown[] }) => { data: unknown }) => void;
    };
    suspenseQuery.mockImplementation((options) => {
      if (options.queryKey[1] === "me") return { data: { display_name: "Ada", email: "ada@example.com" } };
      if (options.queryKey[1] === "workspaces") {
        return { data: [{ id: "w1", slug: "acme", name: "Acme" }] };
      }
      return { data: [{ id: "p1", name: "Atlas", identifier: "ATL", ...harness.projectViews }] };
    });
    const plainQuery = vi.mocked(useQuery) as unknown as { mockImplementation: (run: () => { data: unknown }) => void };
    plainQuery.mockImplementation(() => ({ data: unread }));
  }

  function stubClientQueries(): void {
    createAppClient("https://api.test", (async (url: string) => {
      harness.fetched.push(url);
      if (url.endsWith("/api/users/me/workspaces/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify([
              {
                id: "123e4567-e89b-12d3-a456-426614174000",
                name: "Acme",
                slug: "acme",
                logo_url: null,
                total_members: 1,
                role: harness.role,
              },
            ]),
        };
      }
      if (url.endsWith("/api/instances/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify({
              config: {
                enable_signup: true,
                is_workspace_creation_disabled: harness.creationDisabled,
                is_google_enabled: false,
                is_github_enabled: false,
                is_gitlab_enabled: false,
                is_gitea_enabled: false,
                is_magic_login_enabled: false,
                is_email_password_enabled: true,
                is_smtp_configured: false,
                is_self_managed: true,
              },
            }),
        };
      }
      // The members read 404s: the create dialog renders with no lead list.
      return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
    }) as never);
  }

  function renderShell(): void {
    const Component = Route.options.component;
    if (!Component) throw new Error("gated layout has no component");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    mounted = render(
      <QueryClientProvider client={queryClient}>
        <Component />
      </QueryClientProvider>
    );
  }

  function commandIds(): string[] {
    return listCommands().map((command) => command.id);
  }

  function runCommand(id: string): void {
    const command =
      harness.commands.find((entry) => entry.id === id) ?? listCommands().find((entry) => entry.id === id);
    if (!command) throw new Error(`expected the ${id} command`);
    command.run();
  }

  function resetChrome(): void {
    useShellChrome.setState({ sidebarCollapsed: false, peekOpen: false });
    useRailState.setState({ density: "labeled", undockedByWorkspace: {} });
    globalThis.localStorage.clear();
    delete edition.flags["app-rail"];
    delete edition.flags["sidebar-toggle"];
    delete edition.rail;
    harness.topBar = null;
    harness.sidebarCollapsed = false;
    harness.rail = undefined;
    harness.routerState = { matches: [{ params: {} }], location: { pathname: "/acme/projects/p1/issues" } };
  }

  beforeEach(() => {
    navigate.mockClear();
    harness.commands = [];
    harness.landing = "/";
    harness.signOutError = null;
    harness.settled = false;
    harness.searchScope = null;
    harness.searchBoxScope = null;
    harness.onSelectSearchResult = null;
    harness.role = 15;
    harness.creationDisabled = false;
    harness.projectViews = {
      cycle_view: true,
      module_view: true,
      issue_views_view: true,
      page_view: true,
      inbox_view: true,
    };
    harness.fetched = [];
    harness.navigated = [];
    resetChrome();
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    vi.spyOn(Route, "useNavigate").mockReturnValue(navigate as never);
    vi.spyOn(Route, "useRouteContext").mockReturnValue({ apiClient: {} } as never);
    stubShellQueries();
    stubClientQueries();
  });

  afterEach(() => {
    clearCommands();
    clearPalettePages();
    // Unmount before the store reset: the reset notifies subscribers,
    // and a subscriber rendering after the spies come down throws.
    mounted?.unmount();
    mounted = null;
    vi.restoreAllMocks();
    resetChrome();
  });

  it("registers the shell sign-out command naming the signed-in user", () => {
    renderShell();
    expect(harness.commands).toHaveLength(1);
    expect(harness.commands[0]).toMatchObject({
      id: "shell.sign-out",
      title: "Sign out",
      hint: "Ada",
      section: "Account",
    });
  });

  it("registers the full entry catalog for a member outside a project", async () => {
    renderShell();
    await waitFor(() =>
      expect(commandIds()).toEqual([
        "shell.create-project",
        "shell.create-workspace",
        "shell.go-home",
        "shell.go-projects",
        "shell.go-archives",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
  });

  it("registers the project jumps inside a project without the archives jump", async () => {
    harness.routerState.matches = [{ params: {} }, { params: { projectId: "p1" } }];
    renderShell();
    await waitFor(() =>
      expect(commandIds()).toEqual([
        "shell.create-project",
        "shell.create-workspace",
        "shell.go-cycles",
        "shell.go-home",
        "shell.go-intake",
        "shell.go-modules",
        "shell.go-pages",
        "shell.go-views",
        "shell.go-projects",
        "shell.go-work-items",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
  });

  it("hides flag-gated project jumps when their tabs are off", async () => {
    harness.routerState.matches = [{ params: {} }, { params: { projectId: "p1" } }];
    harness.projectViews = {
      cycle_view: false,
      module_view: false,
      issue_views_view: false,
      page_view: false,
      inbox_view: false,
    };
    renderShell();
    await waitFor(() =>
      expect(commandIds()).toEqual([
        "shell.create-project",
        "shell.create-workspace",
        "shell.go-home",
        "shell.go-projects",
        "shell.go-work-items",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
  });

  it("hides create-gated entries from guests but keeps the workspace entry (SHELL-092)", async () => {
    harness.role = 5;
    renderShell();
    await waitFor(() => expect(harness.fetched.some((url) => url.endsWith("/api/users/me/workspaces/"))).toBe(true));
    await waitFor(() =>
      expect(commandIds()).toEqual([
        "shell.create-workspace",
        "shell.go-home",
        "shell.go-projects",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
  });

  it("falls back to the slug when the workspace row is missing", async () => {
    vi.mocked(Route.useParams).mockReturnValue({ ws: "other" });
    renderShell();
    await waitFor(() => expect(commandIds()).toContain("shell.go-home"));
    const home = listCommands().find((command) => command.id === "shell.go-home");
    expect(home?.hint).toBe("other");
    expect(commandIds()).not.toContain("shell.create-project");
  });

  it("hides the workspace entry when the instance disables creation (SHELL-092)", async () => {
    harness.creationDisabled = true;
    renderShell();
    await waitFor(() =>
      expect(commandIds()).toEqual([
        "shell.create-project",
        "shell.go-home",
        "shell.go-projects",
        "shell.go-archives",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
  });

  it("registers the picker sub-pages", async () => {
    renderShell();
    await waitFor(() => expect(findPalettePage("shell.project-picker")).toBeDefined());
    expect(findPalettePage("shell.workspace-picker")).toBeDefined();
    expect(findPalettePage("shell.theme-picker")).toBeDefined();
    expect(findPalettePage("shell.timezone-picker")).toBeDefined();
    expect(findPalettePage("shell.first-day-picker")).toBeDefined();
    expect(findPalettePage("shell.language-picker")).toBeDefined();
  });

  it("opens the project creation dialog from its entry", async () => {
    renderShell();
    await waitFor(() => expect(commandIds()).toContain("shell.create-project"));
    expect(screen.queryByRole("heading", { name: "New project" })).toBeNull();
    runCommand("shell.create-project");
    expect(await screen.findByRole("heading", { name: "New project" })).not.toBeNull();
  });

  it("mounts the deactivate dialog lazily from the sidebar entry", async () => {
    renderShell();
    const user = userEvent.setup();
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Deactivate account" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(/only way back/)).toBeInTheDocument();
    expect(screen.getByText("ada@example.com")).toBeInTheDocument();
  });

  it("routes the creation, navigation and invite entries", async () => {
    renderShell();
    await waitFor(() => expect(commandIds()).toContain("shell.create-workspace"));
    runCommand("shell.create-workspace");
    expect(navigate).toHaveBeenLastCalledWith({ href: "/create-workspace" });
    runCommand("shell.go-home");
    expect(navigate).toHaveBeenLastCalledWith({ href: "/acme" });
    runCommand("shell.go-projects");
    expect(navigate).toHaveBeenLastCalledWith({ href: "/acme/projects" });
    runCommand("shell.workspace-invites");
    expect(navigate).toHaveBeenLastCalledWith({ href: "/invitations" });
  });

  it("opens help links outside the app", async () => {
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    renderShell();
    await waitFor(() => expect(commandIds()).toContain("shell.docs"));
    runCommand("shell.docs");
    expect(openExternal).toHaveBeenLastCalledWith("https://github.com/The-AI-Republic/pi-dash#readme");
  });

  it("picks navigate through the registered picker pages", async () => {
    renderShell();
    await waitFor(() => expect(findPalettePage("shell.project-picker")).toBeDefined());
    const projectPage = findPalettePage("shell.project-picker");
    const workspacePage = findPalettePage("shell.workspace-picker");
    if (!projectPage || !workspacePage) throw new Error("expected the picker pages");
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { unmount } = render(
      <QueryClientProvider client={queryClient}>
        <PageProbe page={projectPage} query="" />
      </QueryClientProvider>
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Atlas" }));
    expect(navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/issues",
      params: { ws: "acme", projectId: "p1" },
      search: {},
    });
    unmount();
    render(
      <QueryClientProvider client={queryClient}>
        <PageProbe page={workspacePage} query="" />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Acme" }));
    expect(navigate).toHaveBeenCalledWith({ to: "/$ws", params: { ws: "acme" } });
  });

  it("signs the sidebar entry out through the shared action into its landing", async () => {
    renderShell();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sign out Ada" }));
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/" }));
  });

  it("runs the same sign-out path from the palette command", async () => {
    harness.landing = "/login";
    renderShell();
    const command = harness.commands[0];
    if (!command) throw new Error("expected the sign-out command");
    command.run();
    await waitFor(() => expect(navigate).toHaveBeenCalledWith({ href: "/login" }));
  });

  it("stays put when sign-out fails", async () => {
    harness.signOutError = new Error("boom");
    renderShell();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Sign out Ada" }));
    await waitFor(() => expect(harness.settled).toBe(true));
    expect(navigate).not.toHaveBeenCalled();
  });

  it("passes the workspace scope to the shell and the top bar", () => {
    renderShell();
    expect(harness.searchScope).toEqual({ workspaceSlug: "acme", projectId: null });
    expect(harness.searchBoxScope).toEqual({ workspaceSlug: "acme", projectId: null });
  });

  it("scopes the palette search to the child project", () => {
    harness.routerState = {
      matches: [{ params: {} }, { params: { projectId: "p1" } }],
      location: { pathname: "/acme/projects/p1/issues" },
    };
    renderShell();
    expect(harness.searchScope).toEqual({ workspaceSlug: "acme", projectId: "p1" });
    expect(harness.searchBoxScope).toEqual({ workspaceSlug: "acme", projectId: "p1" });
  });

  it("navigates palette server hits through the shared resolver", () => {
    renderShell();
    const select = harness.onSelectSearchResult;
    if (!select) throw new Error("expected the search-result handler");
    select({
      key: "issue:x",
      entity: "issue",
      title: "Fix login",
      path: "ATL-12",
      raw: { sequence_id: 12, project__identifier: "ATL", project_id: "p1" },
    });
    expect(navigate).toHaveBeenCalledWith({
      to: "/$ws/browse/$issueKey",
      params: { ws: "acme", issueKey: "ATL-12" },
    });
  });

  it("wires the top bar with zero counts while the inbox loads", () => {
    renderShell();
    expect(harness.topBar).toMatchObject({
      workspaceSlug: "acme",
      section: "",
      currentPath: "/acme/projects/p1/issues",
      sidebarCollapsed: false,
      sidebarToggleEnabled: true,
      unreadTotal: 0,
      unreadMentions: 0,
    });
    expect(harness.sidebarCollapsed).toBe(false);
  });

  it("names the active project section from the route match", () => {
    harness.routerState = {
      matches: [{ params: { projectId: "p1" } }],
      location: { pathname: "/acme/projects/p1/issues" },
    };
    renderShell();
    expect(harness.topBar?.section).toBe("Atlas");
  });

  it("forwards the unread count once loaded", () => {
    stubShellQueries({ total_unread_notifications_count: 4, mention_unread_notifications_count: 1 });
    renderShell();
    expect(harness.topBar).toMatchObject({ unreadTotal: 4, unreadMentions: 1 });
  });

  it("toggles collapse and clears peek through the bar", () => {
    useShellChrome.setState({ peekOpen: true, sidebarCollapsed: false });
    renderShell();
    expect(harness.topBar).not.toBeNull();
    act(() => {
      harness.topBar?.onToggleSidebar();
    });
    expect(useShellChrome.getState().peekOpen).toBe(false);
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    act(() => {
      harness.topBar?.onToggleSidebar();
    });
    expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
  });

  it("hides the toggle when the edition gates it", () => {
    edition.flags["sidebar-toggle"] = false;
    renderShell();
    expect(harness.topBar?.sidebarToggleEnabled).toBe(false);
  });

  it("suppresses the rail on OSS builds", () => {
    edition.rail = [{ id: "projects", label: "Projects", to: "/acme/" }];
    renderShell();
    expect(harness.rail).toBeUndefined();
  });

  it("mounts the rail with edition dock items once enabled", () => {
    edition.flags["app-rail"] = true;
    edition.rail = [{ id: "projects", label: "Projects", to: "/acme/" }];
    renderShell();
    const rail = harness.rail as React.ReactElement<{
      workspaceSlug: string;
      currentPath: string;
      dockItems: Array<{ id: string; label: string; to: string; icon: React.ReactNode }>;
    }>;
    expect(rail.props.workspaceSlug).toBe("acme");
    expect(rail.props.currentPath).toBe("/acme/projects/p1/issues");
    expect(rail.props.dockItems).toHaveLength(1);
    expect(rail.props.dockItems[0]).toMatchObject({ id: "projects", label: "Projects", to: "/acme/" });
    expect(rail.props.dockItems[0]?.icon).toBeDefined();
  });

  it("drops the rail once the workspace undocks it", () => {
    edition.flags["app-rail"] = true;
    useRailState.setState({ undockedByWorkspace: { acme: true } });
    renderShell();
    expect(harness.rail).toBeUndefined();
  });
});
