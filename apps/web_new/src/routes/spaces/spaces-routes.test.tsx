// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient } from "@tanstack/react-query";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../../features/spaces/api/client.js", () => ({ spaceRuntime: runtime }));

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { routeTree } from "../../routeTree.gen.js";

const UUID = "11111111-1111-4111-8111-111111111111";
const ISSUE_UUID = "22222222-2222-4222-8222-222222222222";

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

const settings = {
  id: UUID,
  project_details: {
    id: UUID,
    identifier: "SPC",
    name: "Public board",
    cover_image: null,
    cover_image_url: null,
    logo_props: {},
    description: "",
    is_default: false,
  },
  workspace_detail: { name: "Acme", slug: "acme", id: UUID, logo_url: null },
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  deleted_at: null,
  created_by: null,
  updated_by: null,
  workspace: UUID,
  project: UUID,
  entity_identifier: "project",
  entity_name: "project",
  anchor: "anchor-1",
  is_comments_enabled: false,
  is_reactions_enabled: false,
  is_votes_enabled: false,
  view_props: { list: true, kanban: true },
  is_disabled: false,
  intake: null,
};

const instance = {
  config: {
    enable_signup: true,
    is_workspace_creation_disabled: false,
    is_google_enabled: false,
    is_github_enabled: false,
    is_gitlab_enabled: false,
    is_gitea_enabled: false,
    is_magic_login_enabled: false,
    is_email_password_enabled: true,
    is_smtp_configured: false,
    is_self_managed: true,
    instance_changelog_url: "",
  },
  instance: {
    id: UUID,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    instance_name: "Pi Dash",
    whitelist_emails: null,
    instance_id: "instance-1",
    current_version: "1.0.0",
    latest_version: null,
    edition: "oss",
    domain: "example.test",
    last_checked_at: "2026-01-01T00:00:00Z",
    namespace: null,
    is_telemetry_enabled: true,
    is_support_required: false,
    is_setup_done: true,
    is_signup_screen_visited: false,
    is_verified: false,
    is_test: false,
    is_current_version_deprecated: false,
    workspaces_exist: true,
  },
};

function renderSpace(
  path: string,
  overrides: { settings?: () => Awaited<ReturnType<Transport>>; entries?: string[] } = {}
) {
  vi.stubGlobal("scrollTo", vi.fn());
  const transport: Transport = async (url) => {
    if (url.includes("/api/instances/")) return reply(200, instance);
    if (url.includes("/settings/")) return overrides.settings?.() ?? reply(200, settings);
    if (new RegExp(`/issues/${ISSUE_UUID}/`).test(url)) {
      return reply(200, {
        id: ISSUE_UUID,
        name: "Open issue",
        state_id: null,
        sort_order: 1,
        estimate_point: null,
        priority: null,
        start_date: null,
        target_date: null,
        sequence_id: 1,
        project_id: UUID,
        parent_id: null,
        created_by: null,
        state__group: null,
        cycle_id: null,
        assignee_ids: [],
        label_ids: [],
        module_ids: [],
        vote_items: [],
        reaction_items: [],
        description_json: null,
        description_html: "",
        description_stripped: "",
        description_binary: null,
      });
    }
    if (url.includes("/issues/")) {
      return reply(200, {
        grouped_by: "state_id",
        total_count: 0,
        next_cursor: null,
        next_page_results: false,
        results: {},
      });
    }
    if (url.includes("/states/")) return reply(200, []);
    if (url.includes("/labels/")) return reply(200, []);
    if (url.includes("/meta/")) return reply(200, { name: "Public board", description: "Team roadmap" });
    if (url.includes("/auth/get-csrf-token/")) return reply(200, { csrf_token: "csrf-test" });
    if (url.includes("/anchor/")) return reply(200, settings);
    throw new Error(`unexpected request ${url}`);
  };
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  createAppClient("https://api.test", transport);
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: overrides.entries ?? [path] }),
    context: {
      queryClient: runtime.queryClient as QueryClient,
      apiClient: runtime.client as ReturnType<typeof createClient>,
    },
    trailingSlash: "preserve",
  });
  render(<RouterProvider router={router} />);
  return router;
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("spaces route tree", () => {
  it("renders a board from the anchor address and settles its search", async () => {
    const router = renderSpace(`/spaces/issues/anchor-1/?board=kanban&peekId=${ISSUE_UUID}&labels=x`);
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(router.state.location.search).toMatchObject({ board: "kanban", peekId: ISSUE_UUID });
  });

  it("rewrites a pasted slash-less board address to the trailing-slash form", async () => {
    const router = renderSpace(`/spaces/issues/anchor-1?board=kanban&peekId=${ISSUE_UUID}&labels=x`);
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/");
    expect(router.state.location.search).toMatchObject({ board: "kanban", peekId: ISSUE_UUID });
  });

  it("keeps the fragment when rewriting a slash-less board address", async () => {
    const router = renderSpace("/spaces/issues/anchor-1?board=list#discussion");
    await waitFor(() => expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/"));
    expect(router.state.location.hash).toBe("discussion");
    expect(router.state.location.search).toEqual({ board: "list" });
  });

  it("renders the anchor illustration for an invalid anchor without a settings read", async () => {
    renderSpace("/spaces/issues/bad%20anchor/");
    expect(await screen.findByTestId("space-anchor-not-found")).toBeInTheDocument();
  });

  it("resolves a legacy address through the space route", async () => {
    const router = renderSpace(`/spaces/acme/${UUID}/?board=list`);
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/");
  });

  it("renders the shared not-found page for an unmatched public path", async () => {
    renderSpace("/spaces/no-such-page/");
    expect(await screen.findByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
    expect(screen.queryByTestId("space-navbar-name")).not.toBeInTheDocument();
  });

  it("renders the landing composition point at the public root", async () => {
    renderSpace("/spaces/");
    expect(await screen.findByTestId("space-landing")).toBeInTheDocument();
  });

  it("hands the decoded landing search to the auth gate", async () => {
    renderSpace("/spaces/?next_path=%2Fissues%2Fanchor-1&email=ada%40example.com&error_code=5075");
    expect(await screen.findByTestId("space-auth-card")).toBeInTheDocument();
    expect(screen.getByLabelText("Email address")).toHaveValue("ada@example.com");
    expect(screen.getByTestId("space-auth-banner")).toBeInTheDocument();
  });

  it("claims noindex with the landing title at the public root", async () => {
    renderSpace("/spaces/");
    await screen.findByTestId("space-landing");
    await waitFor(() => expect(document.title).toBe("Pi Dash — Public boards"));
    expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  });

  it("claims the per-board tags with noindex on the board address", async () => {
    renderSpace("/spaces/issues/anchor-1/");
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    await waitFor(() => expect(document.title).toBe("Public board"));
    expect(document.head.querySelector('meta[property="og:title"]')).toHaveAttribute("content", "Public board");
    expect(document.head.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
    expect(document.head.querySelector('meta[property="og:image"]')).toBeNull();
    expect(document.head.querySelector('meta[name="twitter:image"]')).toBeNull();
  });

  it("renders the fixed generic not-found address", async () => {
    renderSpace("/404");
    expect(await screen.findByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
  });
});

describe("space board layout address (SPC-021/022)", () => {
  it("rewrites a missing view to the first enabled one without adding history", async () => {
    const router = renderSpace("/spaces/issues/anchor-1/", { entries: ["/spaces/", "/spaces/issues/anchor-1/"] });
    await waitFor(() => expect(router.state.location.search).toEqual({ board: "list" }));
    expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/");
    // The settlement replaced the board entry: Back reaches the landing page,
    // never the same board without the view parameter.
    expect(router.history.length).toBe(2);
    await act(async () => {
      router.history.back();
    });
    await waitFor(() => expect(screen.getByTestId("space-landing")).toBeInTheDocument());
  });

  it("switches layout with a push so Back returns to the previous view", async () => {
    const router = renderSpace("/spaces/issues/anchor-1/?board=list");
    await waitFor(() => expect(screen.getByTestId("space-layout-switcher")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Board layout" }));
    await waitFor(() => expect(router.state.location.search).toEqual({ board: "kanban" }));
    await act(async () => {
      router.history.back();
    });
    await waitFor(() => expect(router.state.location.search).toEqual({ board: "list" }));
  });
});

describe("space legacy forwarding", () => {
  it("does not forward filter parameters from a legacy link", async () => {
    const router = renderSpace(`/spaces/acme/${UUID}/?board=kanban&peekId=${ISSUE_UUID}&labels=l1&priority=high`);
    await waitFor(() => expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/"));
    expect(router.state.location.search).toEqual({ board: "kanban", peekId: ISSUE_UUID });
  });
});
