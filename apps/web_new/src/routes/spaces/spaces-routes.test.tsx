// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { createClient, type Transport } from "@pidash/api-client";
import { QueryClient } from "@tanstack/react-query";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../../features/spaces/api/client.js", () => ({ spaceRuntime: runtime }));

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { routeTree } from "../../routeTree.gen.js";

const UUID = "11111111-1111-4111-8111-111111111111";

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
  view_props: {},
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

function renderSpace(path: string, overrides: { settings?: () => Awaited<ReturnType<Transport>> } = {}) {
  vi.stubGlobal("scrollTo", vi.fn());
  const transport: Transport = async (url) => {
    if (url.includes("/api/instances/")) return reply(200, instance);
    if (url.includes("/settings/")) return overrides.settings?.() ?? reply(200, settings);
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
    if (url.includes("/anchor/")) return reply(200, settings);
    throw new Error(`unexpected request ${url}`);
  };
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  createAppClient("https://api.test", transport);
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
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
    const router = renderSpace("/spaces/issues/anchor-1/?board=kanban&peekId=issue-1&labels=x");
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(router.state.location.search).toMatchObject({ board: "kanban", peekId: "issue-1" });
  });

  it("rewrites a pasted slash-less board address to the trailing-slash form", async () => {
    const router = renderSpace("/spaces/issues/anchor-1?board=kanban&peekId=issue-1&labels=x");
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/");
    expect(router.state.location.search).toMatchObject({ board: "kanban", peekId: "issue-1" });
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

  it("renders the fixed generic not-found address", async () => {
    renderSpace("/404");
    expect(await screen.findByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
  });
});

describe("space legacy forwarding", () => {
  it("does not forward filter parameters from a legacy link", async () => {
    const router = renderSpace(`/spaces/acme/${UUID}/?board=kanban&peekId=issue-1&labels=l1&priority=high`);
    await waitFor(() => expect(router.state.location.pathname).toBe("/spaces/issues/anchor-1/"));
    expect(router.state.location.search).toEqual({ board: "kanban", peekId: "issue-1" });
  });
});
