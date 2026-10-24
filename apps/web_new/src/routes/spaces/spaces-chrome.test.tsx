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

const ME = {
  id: UUID,
  email: "ada@example.test",
  username: "ada",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "Ada",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function renderChrome(
  path: string,
  legacy: () => Awaited<ReturnType<Transport>> | Promise<Awaited<ReturnType<Transport>>>,
  viewer: () => Awaited<ReturnType<Transport>> = () => reply(401, { detail: "anonymous" })
): void {
  vi.stubGlobal("scrollTo", vi.fn());
  vi.stubEnv("VITE_WEBSITE_URL", undefined as unknown as string);
  const transport: Transport = async (url) => {
    if (url.includes("/api/instances/")) return reply(200, instance);
    if (url.includes("/settings/")) return reply(200, settings);
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
    if (url.includes("/api/users/me/")) return viewer();
    if (url.includes("/anchor/")) return await legacy();
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
}

afterEach(() => {
  resetAppClient();
  document.documentElement.removeAttribute("lang");
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("spaces chrome", () => {
  it("mounts the navbar chrome, toast host and badge on a settled board", async () => {
    renderChrome("/spaces/issues/anchor-1/?board=kanban&peekId=issue-1", () => reply(200, settings));
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(screen.getByTestId("space-theme-switch")).toBeInTheDocument();
    expect(screen.getByTestId("space-navbar-signin")).toHaveAttribute(
      "href",
      "/spaces/?next_path=/issues/anchor-1/%3Fboard%3Dkanban%26peekId%3Dissue-1"
    );
    await waitFor(() => expect(screen.getByTestId("space-board-empty")).toBeInTheDocument());
    expect(screen.getByTestId("space-product-badge")).toHaveAttribute("href", "https://airepublic.com");
    expect(screen.getByTestId("toast-stack")).toBeInTheDocument();
    expect(screen.queryByTestId("space-route-progress")).not.toBeInTheDocument();
    expect(document.documentElement.getAttribute("lang")).toBe("en");
  });

  it("mounts no badge on the legacy resolution page", async () => {
    let release!: (value: Awaited<ReturnType<Transport>>) => void;
    const held = new Promise<Awaited<ReturnType<Transport>>>((resolve) => {
      release = resolve;
    });
    renderChrome(`/spaces/acme/${UUID}/`, () => held);
    expect(await screen.findByTestId("space-legacy-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("space-product-badge")).not.toBeInTheDocument();
    release(reply(200, settings));
    await waitFor(() => expect(screen.getByTestId("space-navbar-name")).toHaveTextContent("Public board"));
    expect(await screen.findByTestId("space-product-badge")).toBeInTheDocument();
  });

  it("mounts the badge without an account control on the landing auth view", async () => {
    renderChrome("/spaces/", () => reply(200, settings));
    expect(await screen.findByTestId("space-auth-card")).toBeInTheDocument();
    expect(screen.getByTestId("space-product-badge")).toHaveAttribute("href", "https://airepublic.com");
    expect(screen.queryByTestId("space-account-trigger")).not.toBeInTheDocument();
  });

  it("mounts the badge and the account menu on the guidance view when signed in", async () => {
    renderChrome(
      "/spaces/",
      () => reply(200, settings),
      () => reply(200, ME)
    );
    expect(await screen.findByTestId("space-landing-guidance")).toBeInTheDocument();
    expect(screen.getByTestId("space-product-badge")).toBeInTheDocument();
    expect(screen.getByTestId("space-account-trigger")).toHaveTextContent("Ada");
  });
});
