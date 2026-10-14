// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useSuspenseQuery } from "@tanstack/react-query";
import { isRedirect } from "@tanstack/react-router";
import type * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PidashCommand, SearchResultHit } from "../../../shared/commands/index.js";

const harness = vi.hoisted(() => ({
  commands: [] as PidashCommand[],
  landing: "/",
  signOutError: null as Error | null,
  settled: false,
  matches: [{ params: {} }] as Array<{ params: Record<string, unknown> }>,
  searchScope: null as { workspaceSlug: string; projectId: string | null } | null,
  titleBarScope: null as { workspaceSlug: string; projectId: string | null } | null,
  onSelectSearchResult: null as ((hit: SearchResultHit) => void) | null,
}));

// Shell-query stand-in: this suite owns the layout wiring (sign-out entry
// points through the shared action), not data fetching.
vi.mock("@tanstack/react-query", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-query")>();
  return { ...original, useSuspenseQuery: vi.fn() };
});

// Router stand-ins: only the outlet and the matches selector the layout
// reads; route creation, redirects and the Route hooks stay real.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useRouterState: () => harness.matches, Outlet: () => null };
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

// Shell-chrome stand-ins: capture the registered commands and the palette
// search wiring, and expose the sidebar sign-out entry as a button; layout
// and palette stay untested here.
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  const React = await import("react");
  return {
    ...original,
    AppShell: (props: {
      commands?: PidashCommand[];
      topBar?: React.ReactNode;
      sidebar?: React.ReactNode;
      children?: React.ReactNode;
      searchScope: { workspaceSlug: string; projectId: string | null };
      onSelectSearchResult: (hit: SearchResultHit) => void;
    }) => {
      harness.commands = [...(props.commands ?? [])];
      harness.searchScope = props.searchScope;
      harness.onSelectSearchResult = props.onSelectSearchResult;
      return React.createElement("div", null, props.topBar, props.sidebar, props.children);
    },
    Sidebar: (props: { userLabel: string; onSignOut: () => void }) =>
      React.createElement(
        "button",
        { type: "button", "aria-label": `Sign out ${props.userLabel}`, onClick: props.onSignOut },
        "Sign out"
      ),
    TitleBar: (props: { searchScope: { workspaceSlug: string; projectId: string | null } }) => {
      harness.titleBarScope = props.searchScope;
      return null;
    },
  };
});

import { Route } from "./route.js";

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

describe("gated layout shell wiring", () => {
  const navigate = vi.fn();

  function stubShellQueries(): void {
    const suspenseQuery = vi.mocked(useSuspenseQuery) as unknown as {
      mockImplementation: (run: (options: { queryKey: readonly unknown[] }) => { data: unknown }) => void;
    };
    suspenseQuery.mockImplementation((options) => {
      if (options.queryKey[1] === "me") return { data: { display_name: "Ada", email: "ada@example.com" } };
      if (options.queryKey[1] === "workspaces") {
        return { data: [{ id: "w1", slug: "acme", name: "Acme" }] };
      }
      return { data: [{ id: "p1", name: "Atlas", identifier: "ATL" }] };
    });
  }

  function renderShell(): void {
    const Component = Route.options.component;
    if (!Component) throw new Error("gated layout has no component");
    render(<Component />);
  }

  beforeEach(() => {
    navigate.mockClear();
    harness.commands = [];
    harness.landing = "/";
    harness.signOutError = null;
    harness.settled = false;
    harness.matches = [{ params: {} }];
    harness.searchScope = null;
    harness.titleBarScope = null;
    harness.onSelectSearchResult = null;
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    vi.spyOn(Route, "useNavigate").mockReturnValue(navigate as never);
    vi.spyOn(Route, "useRouteContext").mockReturnValue({ apiClient: {} } as never);
    stubShellQueries();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("registers the shell sign-out command naming the signed-in user", () => {
    renderShell();
    expect(harness.commands).toHaveLength(2);
    expect(harness.commands[0]).toMatchObject({
      id: "shell.sign-out",
      title: "Sign out",
      hint: "Ada",
      section: "Account",
    });
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
    expect(harness.titleBarScope).toEqual({ workspaceSlug: "acme", projectId: null });
  });

  it("scopes the palette search to the child project", () => {
    harness.matches = [{ params: {} }, { params: { projectId: "p1" } }];
    renderShell();
    expect(harness.searchScope).toEqual({ workspaceSlug: "acme", projectId: "p1" });
    expect(harness.titleBarScope).toEqual({ workspaceSlug: "acme", projectId: "p1" });
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
});
