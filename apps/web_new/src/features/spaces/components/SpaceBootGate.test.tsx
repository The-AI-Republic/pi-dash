// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ client: null as unknown, queryClient: null as unknown }));
vi.mock("../api/client.js", () => ({ spaceRuntime: runtime }));

import { createClient } from "@pidash/api-client";

import { SpaceBootGate } from "./SpaceBootGate.js";

function reply(status: number, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status,
    url: "",
    headers: { get: () => "application/json" },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

const bootstrap = {
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
    id: "11111111-1111-4111-8111-111111111111",
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

function renderGate(transport: Transport): void {
  runtime.client = createClient({ baseUrl: "https://api.test", transport });
  runtime.queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={runtime.queryClient as QueryClient}>
      <SpaceBootGate>
        <p>board content</p>
      </SpaceBootGate>
    </QueryClientProvider>
  );
}

describe("SpaceBootGate", () => {
  it("shows a full-viewport spinner while the instance probe is pending", () => {
    renderGate(() => new Promise(() => undefined));
    expect(screen.getByTestId("space-boot-loader")).toBeInTheDocument();
    expect(screen.queryByText("board content")).not.toBeInTheDocument();
  });

  it("renders the space instance-failure screen on a failed probe", async () => {
    renderGate(() => Promise.resolve(reply(500, { detail: "down" })));
    expect(await screen.findByTestId("space-instance-failure")).toBeInTheDocument();
    expect(screen.queryByText("board content")).not.toBeInTheDocument();
  });

  it("renders the same failure screen while setup is unfinished", async () => {
    renderGate(() =>
      Promise.resolve(reply(200, { config: {}, instance: { is_activated: false, is_setup_done: false } }))
    );
    expect(await screen.findByTestId("space-instance-failure")).toBeInTheDocument();
  });

  it("hands a ready instance to the board", async () => {
    renderGate(() => Promise.resolve(reply(200, bootstrap)));
    expect(await screen.findByText("board content")).toBeInTheDocument();
    expect(screen.queryByTestId("space-boot-loader")).not.toBeInTheDocument();
  });
});
