// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { isRedirect } from "@tanstack/react-router";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { Suspense } from "react";
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
  suppressPageShortcuts: undefined as boolean | undefined,
  topBar: null as null | {
    workspaceSlug: string;
    section: string;
    currentPath: string;
    sidebarMounted: boolean;
    sidebarCollapsed: boolean;
    sidebarToggleEnabled: boolean;
    onToggleSidebar: () => void;
    unreadTotal: number;
    unreadMentions: number;
    workspaceMenu: unknown;
    helpMenu: unknown;
    accountMenu: unknown;
  },
  sidebar: null as null | {
    personalEntries: unknown;
    editionEntries: unknown;
    currentPath: string;
    onPersonalize: () => void;
    quickActions: unknown;
    projectsGroup: unknown;
    moreGroup: unknown;
    favoritesGroup: unknown;
    onNavigateHref: (href: string) => void;
    workspaces: unknown;
    creationDisabled: unknown;
    userRole: unknown;
    onSelectWorkspace: (slug: string) => void;
    onOpenSettings: (slug: string) => void;
    onInviteMembers: (slug: string) => void;
    onCreateWorkspace: () => void;
    onWorkspaceInvites: () => void;
    onYourWork: () => void;
    onUserSettings: () => void;
  },
  updateControl: undefined as React.ReactNode,
  planBadge: undefined as React.ReactNode,
  createEntry: null as null | {
    workspaceSlug: string;
    projects: Array<{ id: string; name: string; memberRole: number | null; archived: boolean }>;
    canCreate: boolean;
    initialProjectId: string | null;
  },
  personalizeDialog: null as null | {
    open: boolean;
    workspaceSlug: string;
    personal: unknown;
  },
  updatesDialog: null as null | {
    open: boolean;
    changelogUrl: unknown;
    selfManaged: unknown;
  },
  stampError: null as Error | null,
  stamped: [] as Array<unknown>,
  changelogUrl: "",
  selfManaged: false,
  instanceDown: false,
  rail: undefined as unknown,
  routerState: {
    matches: [{ params: {}, routeId: "/$ws/_gated" }],
    location: { pathname: "/acme/projects/p1/issues" },
  } as {
    matches: Array<{ params: Record<string, unknown>; routeId: string }>;
    location: { pathname: string };
  },
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
// the action and command descriptors under test stay real. The profile
// stamp resolves too (or throws), recording its input.
vi.mock("../../../features/auth/signOut.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../features/auth/signOut.js")>();
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
vi.mock("../../../features/auth/profile.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../features/auth/profile.js")>();
  return {
    ...original,
    useUpdateProfile: () => ({
      mutateAsync: async (input: unknown) => {
        harness.stamped.push(input);
        if (harness.stampError !== null) throw harness.stampError;
        return {};
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

// Personalization stand-in: capture the dialog props; the dialog's own
// suite owns its behavior.
vi.mock("../../../features/sidebar/components/PersonalizeDialog.js", async () => {
  const React = await import("react");
  return {
    PersonalizeDialog: (props: { open: boolean; workspaceSlug: string; personal: unknown }) => {
      harness.personalizeDialog = { ...props };
      if (!props.open) return null;
      return React.createElement("div", { "data-testid": "personalize-dialog-standin" });
    },
  };
});

// Workspace create-entry stand-in: capture the props the shell hands it;
// the entry's own suite owns registration and dialog behavior.
vi.mock("../../../features/issues/components/WorkItemCreateEntry.js", async () => {
  const React = await import("react");
  return {
    WorkItemCreateEntry: (props: Record<string, unknown>) => {
      harness.createEntry = props as (typeof harness)["createEntry"];
      return React.createElement("div", { "data-testid": "create-entry" });
    },
    CREATE_WORK_ITEM_COMMAND_ID: "issues.create-work-item",
  };
});
// Runtime-supervisor stand-in: the supervisor's own suite owns its
// behavior (and it needs router context this suite does not provide).
vi.mock("../../../features/runners/index.js", () => ({
  RuntimeSupervisor: () => null,
}));

// Updates stand-in: capture the dialog props; the dialog's own suite
// owns its behavior.
vi.mock("../../../shared/shell/UpdatesDialog.js", async () => {
  const React = await import("react");
  return {
    UpdatesDialog: (props: { open: boolean; changelogUrl: unknown; selfManaged: unknown }) => {
      harness.updatesDialog = { ...props };
      if (!props.open) return null;
      return React.createElement("div", { "data-testid": "updates-dialog-standin" });
    },
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
    // Router-context wirings live with the layout in the app; this suite
    // mounts the component without a RouterProvider.
    useSessionExpiredRedirect: () => undefined,
    AppShell: (props: {
      commands?: PidashCommand[];
      topBar?: React.ReactNode;
      sidebar?: React.ReactNode;
      rail?: React.ReactNode;
      dialogs?: React.ReactNode;
      children?: React.ReactNode;
      searchScope: { workspaceSlug: string; projectId: string | null };
      onSelectSearchResult: (hit: SearchResultHit) => void;
      suppressPageShortcuts?: boolean;
    }) => {
      harness.commands = [...(props.commands ?? [])];
      harness.suppressPageShortcuts = props.suppressPageShortcuts;
      harness.searchScope = props.searchScope;
      harness.onSelectSearchResult = props.onSelectSearchResult;
      harness.rail = props.rail;
      return React.createElement("div", null, props.topBar, props.sidebar, props.dialogs, props.children);
    },
    Sidebar: (props: {
      userLabel: string;
      onSignOut: () => void;
      onDeactivateAccount: () => void;
      personalEntries: unknown;
      editionEntries: unknown;
      currentPath: string;
      onPersonalize: () => void;
      quickActions: unknown;
      projectsGroup: unknown;
      moreGroup: unknown;
      favoritesGroup: unknown;
      onNavigateHref: (href: string) => void;
      workspaces: unknown;
      creationDisabled: unknown;
      userRole: unknown;
      onSelectWorkspace: (slug: string) => void;
      onOpenSettings: (slug: string) => void;
      onInviteMembers: (slug: string) => void;
      onCreateWorkspace: () => void;
      onWorkspaceInvites: () => void;
      onYourWork: () => void;
      onUserSettings: () => void;
      updateControl: React.ReactNode;
      planBadge?: React.ReactNode;
    }) => {
      harness.sidebar = {
        personalEntries: props.personalEntries,
        editionEntries: props.editionEntries,
        currentPath: props.currentPath,
        onPersonalize: props.onPersonalize,
        quickActions: props.quickActions,
        projectsGroup: props.projectsGroup,
        moreGroup: props.moreGroup,
        favoritesGroup: props.favoritesGroup,
        onNavigateHref: props.onNavigateHref,
        workspaces: props.workspaces,
        creationDisabled: props.creationDisabled,
        userRole: props.userRole,
        onSelectWorkspace: props.onSelectWorkspace,
        onOpenSettings: props.onOpenSettings,
        onInviteMembers: props.onInviteMembers,
        onCreateWorkspace: props.onCreateWorkspace,
        onWorkspaceInvites: props.onWorkspaceInvites,
        onYourWork: props.onYourWork,
        onUserSettings: props.onUserSettings,
      };
      harness.planBadge = props.planBadge;
      harness.updateControl = props.updateControl;
      return React.createElement(
        React.Fragment,
        null,
        React.createElement(
          "button",
          { type: "button", "aria-label": `Sign out ${props.userLabel}`, onClick: props.onSignOut },
          "Sign out"
        ),
        React.createElement("button", { type: "button", onClick: props.onDeactivateAccount }, "Deactivate account"),
        React.createElement("button", { type: "button", onClick: props.onPersonalize }, "Personalize")
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
import { selectCreatableProjects } from "../../../features/issues/create/createForm.js";
import { platform } from "../../../core/platform/index.js";
import { DOCS_URL, useRailState, useShellChrome, useShortcutsDialog } from "../../../shared/shell/index.js";
import { isSettingsPagePath, Route } from "./route.js";

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

describe("isSettingsPagePath", () => {
  it("recognizes project settings addresses and nothing else", () => {
    expect(isSettingsPagePath("/acme/settings/projects")).toBe(true);
    expect(isSettingsPagePath("/acme/settings/projects/")).toBe(true);
    expect(isSettingsPagePath("/acme/settings/projects/p1")).toBe(true);
    expect(isSettingsPagePath("/acme/settings/projects/p1/features/cycles/")).toBe(true);
    expect(isSettingsPagePath("/acme/settings/account/security")).toBe(false);
    expect(isSettingsPagePath("/acme/projects/p1/issues")).toBe(false);
    expect(isSettingsPagePath("/acme/projects/p1/settings")).toBe(false);
    expect(isSettingsPagePath("/")).toBe(false);
  });
});

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
    const failed = new ApiError({ status: 401, code: "http", message: "signed out" });
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
  it("starts the projects and favorites fetches, then prefetches workspaces, the unread count and sidebar preferences", async () => {
    // Projects and favorites load eagerly but outside the blocking gate:
    // the sidebar renders placeholder rows while the collection resolves
    // (SHELL-049) and hides favorites until they arrive (SHELL-056).
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
      ["ws", "acme", "projects", "list"],
      ["ws", "acme", "favorites"],
      ["session", "workspaces"],
      ["ws", "acme", "notifications", "unread"],
      ["ws", "acme", "sidebar-preferences"],
    ]);
  });

  it("does not wait for favorites before the shell renders", async () => {
    const args = {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: (options: { queryKey: readonly unknown[] }) =>
            options.queryKey[2] === "favorites" ? new Promise(() => undefined) : Promise.resolve([]),
        },
      },
      params: { ws: "acme" },
    } as never;
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(loader(args)).resolves.toBeUndefined();
  });

  it("still resolves when the favorites prefetch fails", async () => {
    const args = {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            if (options.queryKey[2] === "favorites") throw new Error("favorites down");
            return [];
          },
        },
      },
      params: { ws: "acme" },
    } as never;
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(loader(args)).resolves.toBeUndefined();
  });

  it("still resolves when the unread-count prefetch fails", async () => {
    const args = {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            if (options.queryKey[2] === "notifications") throw new Error("unread down");
            return [];
          },
        },
      },
      params: { ws: "acme" },
    } as never;
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(loader(args)).resolves.toBeUndefined();
  });

  it("still resolves when the projects prefetch fails", async () => {
    const args = {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            if (options.queryKey[2] === "projects") throw new Error("projects down");
            return [];
          },
        },
      },
      params: { ws: "acme" },
    } as never;
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(loader(args)).resolves.toBeUndefined();
  });

  it("still resolves when the sidebar-preferences prefetch fails", async () => {
    const args = {
      context: {
        apiClient: {},
        queryClient: {
          ensureQueryData: async (options: { queryKey: readonly unknown[] }) => {
            if (options.queryKey[2] === "sidebar-preferences") throw new Error("prefs down");
            return [];
          },
        },
      },
      params: { ws: "acme" },
    } as never;
    const loader = Route.options.loader;
    if (typeof loader !== "function") throw new Error("missing loader");
    await expect(loader(args)).resolves.toBeUndefined();
  });
});

describe("gated layout shell wiring", () => {
  const navigate = vi.fn();
  let mounted: { unmount: () => void } | null = null;

  function stubShellQueries(
    unread: unknown = undefined,
    sidebarPrefs: unknown = { data: undefined },
    projectsResult: unknown = null,
    meResult: { data: unknown } = {
      data: { id: "u1", display_name: "Ada", email: "ada@example.com", avatar_url: null },
    }
  ): void {
    const suspenseQuery = vi.mocked(useSuspenseQuery) as unknown as {
      mockImplementation: (run: (options: { queryKey: readonly unknown[] }) => { data: unknown }) => void;
    };
    suspenseQuery.mockImplementation((options) => {
      if (options.queryKey[1] === "me") return meResult;
      if (options.queryKey[1] === "workspaces") {
        return {
          data: [{ id: "w1", slug: "acme", name: "Acme", role: harness.role, total_members: 3, logo_url: null }],
        };
      }
      throw new Error(`unexpected suspense query ${JSON.stringify(options.queryKey)}`);
    });
    // Plain queries serve the projects collection (never blocking: the
    // shell renders while it loads), the unread dot (default) plus the
    // membership and instance reads the palette entries perform through
    // the session hooks. The entries' own suite proves the real fetch;
    // here the mock stands in with the same shapes the fetch stub below
    // serves, and records the membership read the guest test waits on.
    // Sidebar preferences ride the same mock with a per-test result.
    const plainQuery = vi.mocked(useQuery) as unknown as {
      mockImplementation: (run: (options: { queryKey: readonly unknown[] }) => unknown) => void;
    };
    plainQuery.mockImplementation((options) => {
      // The plan badge rides the lazy chunk below the shell; serve it a
      // free plan so the test can see the resolved label.
      if (options.queryKey[0] === "cloud-billing") {
        return { data: { is_paid: false, migration_required: false, user_center_url: "https://home.test/account" } };
      }
      if (options.queryKey[0] === "ws" && options.queryKey[2] === "projects" && options.queryKey[3] === "list") {
        if (projectsResult !== null) return projectsResult;
        return {
          data: [
            {
              id: "p1",
              name: "Atlas",
              identifier: "ATL",
              sort_order: 10000,
              member_role: 20,
              intake_count: 0,
              archived_at: null,
              ...harness.projectViews,
            },
          ],
          status: "success",
        };
      }
      if (options.queryKey[0] === "ws" && options.queryKey[2] === "sidebar-preferences") return sidebarPrefs;
      if (options.queryKey[0] === "session" && options.queryKey[1] === "workspaces") {
        harness.fetched.push("https://api.test/api/users/me/workspaces/");
        return { data: [{ id: "w1", slug: "acme", name: "Acme", role: harness.role }] };
      }
      if (options.queryKey[0] === "instance") {
        if (harness.instanceDown) return { data: undefined };
        return {
          data: {
            is_workspace_creation_disabled: harness.creationDisabled,
            instance_changelog_url: harness.changelogUrl,
            is_self_managed: harness.selfManaged,
          },
        };
      }
      return { data: unread };
    });
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
      if (url.endsWith("/api/billing/me/plan/")) {
        return {
          status: 200,
          url,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify({
              is_paid: false,
              migration_required: false,
              user_center_url: "https://home.test/account",
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
    useShellChrome.setState({ sidebarCollapsed: false, peekOpen: false, sidebarWidth: 256 });
    useRailState.setState({ density: "labeled", undockedByWorkspace: {} });
    globalThis.localStorage.clear();
    delete edition.flags["app-rail"];
    delete edition.flags["sidebar-toggle"];
    delete edition.flags["plan-badge"];
    delete edition.rail;
    delete edition.sidebar;
    harness.topBar = null;
    harness.sidebar = null;
    harness.personalizeDialog = null;
    harness.createEntry = null;
    harness.updatesDialog = null;
    harness.rail = undefined;
    harness.suppressPageShortcuts = undefined;
    harness.routerState = {
      matches: [{ params: {}, routeId: "/$ws/_gated" }],
      location: { pathname: "/acme/projects/p1/issues" },
    };
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
    harness.changelogUrl = "";
    harness.selfManaged = false;
    harness.instanceDown = false;
    harness.stampError = null;
    harness.stamped = [];
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
    vi.spyOn(Route, "useRouteContext").mockReturnValue({ apiClient: { baseUrl: "https://api.test" } } as never);
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

  it("suppresses page shortcuts on settings addresses only", () => {
    renderShell();
    expect(harness.suppressPageShortcuts).toBe(false);
  });

  it("pins key handling off under the project settings area", () => {
    harness.routerState = {
      matches: [{ params: {}, routeId: "/$ws/_gated" }],
      location: { pathname: "/acme/settings/projects/p1/states/" },
    };
    renderShell();
    expect(harness.suppressPageShortcuts).toBe(true);
  });

  it.each(["/acme/settings/projects", "/acme/settings/projects/p1"])(
    "pins key handling off on the no-slash settings address %s",
    (pathname) => {
      harness.routerState = {
        matches: [{ params: {}, routeId: "/$ws/_gated" }],
        location: { pathname },
      };
      renderShell();
      expect(harness.suppressPageShortcuts).toBe(true);
    }
  );

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
        "shell.go-drafts",
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
    harness.routerState.matches = [
      { params: {}, routeId: "/$ws/_gated" },
      { params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" },
    ];
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
    harness.routerState.matches = [
      { params: {}, routeId: "/$ws/_gated" },
      { params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" },
    ];
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
        "shell.go-drafts",
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

  it("labels the account by email when the profile carries no display name", () => {
    stubShellQueries(undefined, { data: undefined }, null, {
      data: { id: "u1", display_name: "", email: "ada@example.com", avatar_url: null },
    });
    renderShell();
    expect(screen.getByRole("button", { name: "Sign out ada@example.com" })).toBeInTheDocument();
  });

  it("passes the workspace scope to the shell and the top bar", () => {
    renderShell();
    expect(harness.searchScope).toEqual({ workspaceSlug: "acme", projectId: null });
    expect(harness.searchBoxScope).toEqual({ workspaceSlug: "acme", projectId: null });
  });

  it("scopes the palette search to the child project", () => {
    harness.routerState = {
      matches: [
        { params: {}, routeId: "/$ws/_gated" },
        { params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" },
      ],
      location: { pathname: "/acme/projects/p1/issues" },
    };
    renderShell();
    expect(harness.searchScope).toEqual({ workspaceSlug: "acme", projectId: "p1" });
    expect(harness.searchBoxScope).toEqual({ workspaceSlug: "acme", projectId: "p1" });
  });

  it("navigates palette server hits through the shared resolver", async () => {
    renderShell();
    const select = harness.onSelectSearchResult;
    if (!select) throw new Error("expected the search-result handler");
    // The resolver rides a dynamic import off the shell chunk
    // (NEWFRONT-871, size gate), so the navigate lands a tick later.
    select({
      key: "issue:x",
      entity: "issue",
      title: "Fix login",
      path: "ATL-12",
      raw: { sequence_id: 12, project__identifier: "ATL", project_id: "p1" },
    });
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({
        to: "/$ws/browse/$issueKey",
        params: { ws: "acme", issueKey: "ATL-12" },
      })
    );
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
  });

  it("hands the window to workspace settings instead of the workspace chrome", () => {
    harness.routerState = {
      matches: [
        { params: {}, routeId: "/$ws/_gated" },
        { params: {}, routeId: "/$ws/_gated/settings/_workspace" },
        { params: {}, routeId: "/$ws/_gated/settings/_workspace/members/" },
      ],
      location: { pathname: "/acme/settings/members/" },
    };
    renderShell();
    expect(harness.topBar).toBeNull();
    expect(harness.sidebar).toBeNull();
    // The settings pages keep the workspace wirings AppShell hosts
    // (session-expiry redirect and the toast host), without the chrome.
    expect(screen.getByRole("region", { name: "Notifications" })).toBeInTheDocument();
  });

  it("keeps the workspace chrome for project settings beneath the same prefix", () => {
    harness.routerState = {
      matches: [
        { params: {}, routeId: "/$ws/_gated" },
        { params: { projectId: "p1" }, routeId: "/$ws/_gated/settings/projects/$projectId/labels" },
      ],
      location: { pathname: "/acme/settings/projects/p1/labels" },
    };
    renderShell();
    expect(harness.topBar).not.toBeNull();
    expect(harness.sidebar).not.toBeNull();
  });

  it("names the active project section from the route match", () => {
    harness.routerState = {
      matches: [{ params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" }],
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
      dockItems: Array<{ id: string; label: string; to: string }>;
    }>;
    expect(rail.props.workspaceSlug).toBe("acme");
    expect(rail.props.currentPath).toBe("/acme/projects/p1/issues");
    expect(rail.props.dockItems).toHaveLength(1);
    expect(rail.props.dockItems[0]).toMatchObject({ id: "projects", label: "Projects", to: "/acme/" });
  });

  it("drops the rail once the workspace undocks it", () => {
    edition.flags["app-rail"] = true;
    useRailState.setState({ undockedByWorkspace: { acme: true } });
    renderShell();
    expect(harness.rail).toBeUndefined();
  });

  it("loads the rail strip on demand once enabled", async () => {
    edition.flags["app-rail"] = true;
    edition.rail = [{ id: "projects", label: "Projects", to: "/acme/" }];
    renderShell();
    const rail = harness.rail as React.ReactElement;
    expect(rail).toBeDefined();
    const strip = render(<Suspense fallback={null}>{rail}</Suspense>);
    try {
      expect(await screen.findByRole("link", { name: "Settings" })).toHaveAttribute("href", "/acme/settings");
    } finally {
      strip.unmount();
    }
  });

  it("passes personal entries, the current path and the personalization opener to the sidebar", () => {
    renderShell();
    expect(harness.sidebar?.personalEntries).toEqual([{ key: "drafts", label: "Drafts", href: "/acme/drafts/" }]);
    expect(harness.sidebar?.currentPath).toBe("/acme/projects/p1/issues");
    expect(typeof harness.sidebar?.onPersonalize).toBe("function");
  });

  it("passes no edition entries on the OSS edition (CLOUD-021 flag-off)", () => {
    renderShell();
    expect(harness.sidebar?.editionEntries).toEqual([]);
  });

  it("mounts the plan badge lazily (CLOUD-038)", () => {
    renderShell();
    const badge = harness.planBadge;
    expect(React.isValidElement(badge)).toBe(true);
    // The slot only suspends around the split badge; the feature's own
    // suites prove the plan read, labels and focus refresh.
    expect((badge as React.ReactElement).type).toBe(Suspense);
  });

  it("loads the plan badge from its own chunk inside the sidebar slot (CLOUD-038, size gate)", async () => {
    edition.flags["plan-badge"] = true;
    // Installed first: the badge opens through the platform.
    const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
    renderShell();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const slot = render(
      <QueryClientProvider client={queryClient}>
        <Suspense fallback={<span>loading plan</span>}>{harness.planBadge}</Suspense>
      </QueryClientProvider>
    );
    try {
      // Resolving the boundary imports the split chunk and renders the
      // label from the plan the query mock serves.
      const badge = await screen.findByRole("button", { name: "Plan: Upgrade" });
      expect(screen.queryByText("loading plan")).not.toBeInTheDocument();
      // The badge opens through the platform, so the desktop build uses
      // its own window rather than the web page.
      await userEvent.setup().click(badge);
      expect(openExternal).toHaveBeenCalledWith("https://home.test/account");
      openExternal.mockRestore();
    } finally {
      slot.unmount();
    }
  });

  it("fills the shell's sidebar slot with the desktop update control (SHELL-102)", () => {
    renderShell();
    // The shell only owns the mount point; the desktop area supplies the
    // control, which renders nothing on web and with nothing waiting.
    expect(React.isValidElement(harness.updateControl)).toBe(true);
  });

  it("loads the update control from its own chunk inside the sidebar slot (DESK-018)", async () => {
    renderShell();
    const slot = render(<Suspense fallback={<span>loading control</span>}>{harness.updateControl}</Suspense>);
    try {
      // Resolving the lazy factory imports the real chunk; on the web
      // platform it asks for no update and leaves the slot empty.
      await waitFor(() => expect(screen.queryByText("loading control")).not.toBeInTheDocument());
      expect(slot.container).toBeEmptyDOMElement();
    } finally {
      slot.unmount();
    }
  });

  it("resolves edition sidebar entries against the workspace slug (CLOUD-021)", () => {
    edition.sidebar = [{ id: "apps", label: "Apps", to: "/apps/" }];
    renderShell();
    expect(harness.sidebar?.editionEntries).toEqual([{ key: "apps", label: "Apps", href: "/acme/apps/" }]);
  });

  it("keeps edition entries for guests (CLOUD-021)", () => {
    edition.sidebar = [{ id: "apps", label: "Apps", to: "/apps/" }];
    harness.role = 5;
    renderShell();
    expect(harness.sidebar?.editionEntries).toEqual([{ key: "apps", label: "Apps", href: "/acme/apps/" }]);
  });

  it("filters unpinned entries from the sidebar list", () => {
    stubShellQueries(undefined, {
      status: "success",
      data: { drafts: { is_pinned: false, sort_order: 0 } },
    });
    renderShell();
    expect(harness.sidebar?.personalEntries).toEqual([]);
  });

  it("hides member-only sidebar rows from guests (SHELL-046)", () => {
    harness.role = 5;
    renderShell();
    expect(harness.sidebar?.personalEntries).toEqual([]);
    const quickActions = harness.sidebar?.quickActions as React.ReactElement<{ canCreate: boolean }>;
    expect(quickActions.props.canCreate).toBe(false);
    const group = harness.sidebar?.projectsGroup as React.ReactElement<{ canCreateProject: boolean }>;
    expect(group.props.canCreateProject).toBe(false);
  });

  it("passes the more and favorites slots with guest gating (SHELL-053/056)", () => {
    renderShell();
    const more = harness.sidebar?.moreGroup as React.ReactElement<{
      workspaceSlug: string;
      role: number | null;
      currentPath: string;
    }>;
    expect(more.props.workspaceSlug).toBe("acme");
    expect(more.props.role).toBe(15);
    expect(typeof more.props.currentPath).toBe("string");
    expect(harness.sidebar?.favoritesGroup).toBeDefined();
  });

  it("keeps the more disclosure but drops favorites for guests (SHELL-053/056)", () => {
    harness.role = 5;
    renderShell();
    const more = harness.sidebar?.moreGroup as React.ReactElement<{ role: number | null }>;
    expect(more.props.role).toBe(5);
    expect(harness.sidebar?.favoritesGroup).toBeUndefined();
  });

  it("mounts the workspace create entry with the shell's projects and standing", () => {
    harness.routerState = {
      matches: [{ params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" }],
      location: { pathname: "/acme/projects/p1/issues" },
    };
    renderShell();
    expect(harness.createEntry).toEqual({
      workspaceSlug: "acme",
      projects: [{ id: "p1", name: "Atlas", memberRole: 20, archived: false }],
      canCreate: true,
      initialProjectId: "p1",
    });
  });

  it("gives the workspace create entry no page project outside a project route", () => {
    harness.routerState = {
      matches: [{ params: {}, routeId: "/$ws/_gated/drafts" }],
      location: { pathname: "/acme/drafts" },
    };
    renderShell();
    expect(harness.createEntry?.initialProjectId).toBeNull();
    expect(harness.createEntry?.canCreate).toBe(true);
  });

  it("keeps the workspace create entry off for guests even beside a project", () => {
    harness.role = 5;
    harness.routerState = {
      matches: [{ params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" }],
      location: { pathname: "/acme/projects/p1/issues" },
    };
    renderShell();
    expect(harness.createEntry?.canCreate).toBe(false);
  });

  it("passes the quick-create slot its picker and standing (SHELL-047)", () => {
    renderShell();
    const quickActions = harness.sidebar?.quickActions as React.ReactElement<{
      workspaceSlug: string;
      projects: Array<{ id: string; name: string; memberRole: number | null; archived: boolean }>;
      canCreate: boolean;
    }>;
    expect(quickActions.props.workspaceSlug).toBe("acme");
    expect(quickActions.props.projects).toEqual([{ id: "p1", name: "Atlas", memberRole: 20, archived: false }]);
    expect(quickActions.props.canCreate).toBe(true);
  });

  it("carries each project's own role into the quick-create list", () => {
    // The workspace admin is only a guest on the second project, which
    // must not appear as creatable (ISS-117/ISS-123).
    stubShellQueries(
      undefined,
      { data: undefined },
      {
        data: [
          {
            id: "p1",
            name: "Atlas",
            identifier: "ATL",
            sort_order: 10000,
            member_role: 20,
            intake_count: 0,
            archived_at: null,
            ...harness.projectViews,
          },
          {
            id: "p2",
            name: "Guestland",
            identifier: "GST",
            sort_order: 20000,
            member_role: 5,
            intake_count: 0,
            archived_at: null,
            ...harness.projectViews,
          },
        ],
        status: "success",
      }
    );
    harness.role = 20;
    renderShell();
    const quickActions = harness.sidebar?.quickActions as React.ReactElement<{
      projects: Array<{ id: string; memberRole: number | null }>;
    }>;
    expect(quickActions.props.projects).toEqual([
      expect.objectContaining({ id: "p1", memberRole: 20 }),
      expect.objectContaining({ id: "p2", memberRole: 5 }),
    ]);
    const creatable = quickActions.props.projects.map((project) => ({
      ...project,
      name: project.id,
      identifier: project.id.slice(0, 3),
      archived: false,
    }));
    expect(selectCreatableProjects(creatable).map((project) => project.id)).toEqual(["p1"]);
  });

  it("passes the projects group slot its collection and handlers (SHELL-048)", () => {
    harness.routerState = {
      matches: [{ params: { projectId: "p1" }, routeId: "/$ws/_gated/projects/$projectId/issues/" }],
      location: { pathname: "/acme/projects/p1/issues" },
    };
    renderShell();
    const group = harness.sidebar?.projectsGroup as React.ReactElement<{
      workspaceSlug: string;
      projects: unknown[];
      projectsStatus: string;
      currentPath: string;
      activeProjectId: string | null;
      canCreateProject: boolean;
      onCreateProject: () => void;
      onNavigate: (href: string) => void;
    }>;
    expect(group.props.workspaceSlug).toBe("acme");
    expect(group.props.projects).toHaveLength(1);
    expect(group.props.projectsStatus).toBe("success");
    expect(group.props.currentPath).toBe("/acme/projects/p1/issues");
    expect(group.props.activeProjectId).toBe("p1");
    expect(group.props.canCreateProject).toBe(true);
    expect(typeof group.props.onCreateProject).toBe("function");
    expect(typeof group.props.onNavigate).toBe("function");
    expect(typeof harness.sidebar?.onNavigateHref).toBe("function");
  });

  it("forwards the pending collection to the projects group (SHELL-049)", () => {
    stubShellQueries(undefined, { data: undefined }, { data: undefined, status: "pending" });
    renderShell();
    const group = harness.sidebar?.projectsGroup as React.ReactElement<{
      projects: unknown[];
      projectsStatus: string;
    }>;
    expect(group.props.projectsStatus).toBe("pending");
    expect(group.props.projects).toEqual([]);
  });

  it("opens personalization from the sidebar launcher with the resolved group", async () => {
    const user = userEvent.setup();
    stubShellQueries(undefined, {
      status: "success",
      data: { drafts: { is_pinned: true, sort_order: 2 } },
    });
    renderShell();
    expect(screen.queryByTestId("personalize-dialog-standin")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Personalize" }));
    expect(await screen.findByTestId("personalize-dialog-standin")).not.toBeNull();
    expect(harness.personalizeDialog).toMatchObject({ open: true, workspaceSlug: "acme" });
    expect(harness.personalizeDialog?.personal).toEqual({
      status: "ready",
      entries: [{ key: "drafts", label: "Drafts", path: "drafts", enabled: true, sortOrder: 2 }],
    });
  });

  it.each([
    ["pending", { status: "pending", data: undefined }],
    ["error", { status: "error", data: undefined }],
  ])("resolves the %s personal group for the dialog", async (_label, prefs) => {
    const user = userEvent.setup();
    stubShellQueries(undefined, prefs);
    renderShell();
    await user.click(screen.getByRole("button", { name: "Personalize" }));
    expect(await screen.findByTestId("personalize-dialog-standin")).not.toBeNull();
    expect(harness.personalizeDialog?.personal).toEqual({ status: _label, entries: [] });
  });

  describe("chrome menus (SHELL-058-062)", () => {
    function stubNarrowViewport(narrow: boolean): void {
      vi.stubGlobal("matchMedia", () => ({
        matches: narrow,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
      }));
    }

    function renderNode(node: unknown): { unmount: () => void } {
      return render(<Suspense fallback={null}>{node as React.ReactElement}</Suspense>);
    }

    afterEach(() => {
      vi.unstubAllGlobals();
      useShortcutsDialog.getState().closeShortcuts();
    });

    it("passes switcher rows, the creation flag and the role to the sidebar", () => {
      renderShell();
      expect(harness.sidebar?.workspaces).toEqual([
        { id: "w1", slug: "acme", name: "Acme", role: 15, totalMembers: 3, logoSrc: null },
      ]);
      expect(harness.sidebar?.creationDisabled).toBe(false);
      expect(harness.sidebar?.userRole).toBe(15);
      expect(harness.topBar?.sidebarMounted).toBe(true);
    });

    it("hides creation while the instance disables it", () => {
      harness.creationDisabled = true;
      renderShell();
      expect(harness.sidebar?.creationDisabled).toBe(true);
    });

    it("stamps the last workspace, then navigates", async () => {
      renderShell();
      const menu = renderNode(harness.topBar?.workspaceMenu);
      try {
        fireEvent.click(screen.getByTestId("topbar-workspace-switcher"));
        fireEvent.click(screen.getByRole("menuitem", { name: /Acme/ }));
        await waitFor(() => expect(navigate).toHaveBeenCalledWith({ to: "/$ws", params: { ws: "acme" } }));
        expect(harness.stamped).toEqual([{ last_workspace_id: "w1" }]);
      } finally {
        menu.unmount();
      }
    });

    it("still navigates when the stamp fails", async () => {
      harness.stampError = new Error("profile down");
      renderShell();
      const menu = renderNode(harness.topBar?.workspaceMenu);
      try {
        fireEvent.click(screen.getByTestId("topbar-workspace-switcher"));
        fireEvent.click(screen.getByRole("menuitem", { name: /Acme/ }));
        await waitFor(() => expect(navigate).toHaveBeenCalledWith({ to: "/$ws", params: { ws: "acme" } }));
        expect(harness.stamped).toEqual([{ last_workspace_id: "w1" }]);
      } finally {
        menu.unmount();
      }
    });

    it("navigates directly for unknown slugs without stamping", () => {
      renderShell();
      harness.sidebar?.onSelectWorkspace("ghost");
      expect(navigate).toHaveBeenCalledWith({ to: "/$ws", params: { ws: "ghost" } });
      expect(harness.stamped).toEqual([]);
    });

    it("routes the switcher shortcuts to their addresses", () => {
      renderShell();
      harness.sidebar?.onOpenSettings("acme");
      expect(navigate).toHaveBeenLastCalledWith({ href: "/acme/settings" });
      harness.sidebar?.onInviteMembers("acme");
      expect(navigate).toHaveBeenLastCalledWith({ href: "/acme/settings/members" });
      harness.sidebar?.onCreateWorkspace();
      expect(navigate).toHaveBeenLastCalledWith({ href: "/create-workspace" });
      harness.sidebar?.onWorkspaceInvites();
      expect(navigate).toHaveBeenLastCalledWith({ href: "/invitations" });
    });

    it("routes the account shortcuts to the profile and the settings page", () => {
      renderShell();
      harness.sidebar?.onYourWork();
      expect(navigate).toHaveBeenLastCalledWith({ href: "/acme/profile/u1" });
      harness.sidebar?.onUserSettings();
      expect(navigate).toHaveBeenLastCalledWith({ href: "/acme/settings/account/general/" });
    });

    it("falls back to guest without an active membership", () => {
      const suspenseQuery = vi.mocked(useSuspenseQuery) as unknown as {
        mockImplementation: (run: (options: { queryKey: readonly unknown[] }) => { data: unknown }) => void;
      };
      suspenseQuery.mockImplementation((options) => {
        if (options.queryKey[1] === "me") {
          return { data: { id: "u1", display_name: "Ada", email: "ada@example.com", avatar_url: null } };
        }
        if (options.queryKey[1] === "workspaces") return { data: [] };
        return { data: [] };
      });
      renderShell();
      expect(harness.sidebar?.userRole).toBe(5);
      const menu = renderNode(harness.topBar?.accountMenu);
      try {
        fireEvent.click(screen.getByTestId("topbar-user-menu"));
        expect(screen.queryByRole("menuitem", { name: "Your work" })).not.toBeInTheDocument();
      } finally {
        menu.unmount();
      }
    });

    it("unmounts the sidebar on the notifications route with the compact fallback", () => {
      harness.routerState.location.pathname = "/acme/notifications/";
      renderShell();
      expect(harness.sidebar).toBeNull();
      expect(harness.topBar?.sidebarMounted).toBe(false);
      const menu = renderNode(harness.topBar?.accountMenu);
      try {
        expect(screen.getByTestId("topbar-user-menu")).toBeInTheDocument();
      } finally {
        menu.unmount();
      }
    });

    it("opens the shortcut list from the help menu", async () => {
      renderShell();
      const menu = renderNode(harness.topBar?.helpMenu);
      try {
        fireEvent.click(await screen.findByTestId("topbar-help-menu"));
        fireEvent.click(screen.getByRole("menuitem", { name: "Keyboard shortcuts" }));
        expect(useShortcutsDialog.getState().open).toBe(true);
      } finally {
        menu.unmount();
      }
    });

    it("opens the updates dialog with the instance changelog", async () => {
      harness.changelogUrl = "https://changelog.example/feed";
      harness.selfManaged = true;
      renderShell();
      expect(screen.queryByTestId("updates-dialog-standin")).toBeNull();
      const menu = renderNode(harness.topBar?.helpMenu);
      try {
        fireEvent.click(screen.getByTestId("topbar-help-menu"));
        fireEvent.click(screen.getByRole("menuitem", { name: "What's new?" }));
        expect(await screen.findByTestId("updates-dialog-standin")).not.toBeNull();
        expect(harness.updatesDialog).toMatchObject({
          open: true,
          changelogUrl: "https://changelog.example/feed",
          selfManaged: true,
        });
      } finally {
        menu.unmount();
      }
    });

    it("falls back to safe chrome defaults without instance config", async () => {
      harness.instanceDown = true;
      renderShell();
      expect(harness.sidebar?.creationDisabled).toBe(false);
      const menu = renderNode(harness.topBar?.helpMenu);
      try {
        fireEvent.click(screen.getByTestId("topbar-help-menu"));
        fireEvent.click(screen.getByRole("menuitem", { name: "What's new?" }));
        expect(await screen.findByTestId("updates-dialog-standin")).not.toBeNull();
        expect(harness.updatesDialog).toMatchObject({ open: true, changelogUrl: null, selfManaged: false });
      } finally {
        menu.unmount();
      }
    });

    it("opens help links outside the app", () => {
      const openExternal = vi.spyOn(platform, "openExternal").mockResolvedValue(undefined);
      renderShell();
      const menu = renderNode(harness.topBar?.helpMenu);
      try {
        fireEvent.click(screen.getByTestId("topbar-help-menu"));
        fireEvent.click(screen.getByRole("menuitem", { name: "Documentation" }));
        expect(openExternal).toHaveBeenCalledWith(DOCS_URL);
      } finally {
        menu.unmount();
      }
    });

    it("collapses the floating sidebar on shell navigation while narrow", async () => {
      stubNarrowViewport(true);
      renderShell();
      expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
      // Project picks travel through the palette into handleSelectProject.
      await waitFor(() => expect(findPalettePage("shell.project-picker")).toBeDefined());
      const projectPage = findPalettePage("shell.project-picker");
      if (!projectPage) throw new Error("expected the project picker page");
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
      const { unmount } = render(
        <QueryClientProvider client={queryClient}>
          <PageProbe page={projectPage} query="" />
        </QueryClientProvider>
      );
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Atlas" }));
      expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
      expect(navigate).toHaveBeenCalledWith({
        to: "/$ws/projects/$projectId/issues",
        params: { ws: "acme", projectId: "p1" },
        search: {},
      });
      unmount();
      useShellChrome.getState().setSidebarCollapsed(false);
      harness.sidebar?.onYourWork();
      expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
      useShellChrome.getState().setSidebarCollapsed(false);
      harness.onSelectSearchResult?.({
        key: "issue:x",
        entity: "issue",
        title: "Hit",
        path: "",
        raw: {
          id: "22222222-2222-4222-8222-222222222222",
          sequence_id: 12,
          project__identifier: "ATL",
          project_id: "11111111-1111-4111-8111-111111111111",
        },
      });
      expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
      // The resolver chunk loads before the navigate (NEWFRONT-871).
      await waitFor(() =>
        expect(navigate).toHaveBeenCalledWith({
          to: "/$ws/browse/$issueKey",
          params: { ws: "acme", issueKey: "ATL-12" },
        })
      );
    });

    it("leaves the sidebar open navigating while wide", () => {
      stubNarrowViewport(false);
      renderShell();
      harness.sidebar?.onYourWork();
      expect(useShellChrome.getState().sidebarCollapsed).toBe(false);
    });
  });
});
