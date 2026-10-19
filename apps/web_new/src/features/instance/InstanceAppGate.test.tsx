// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { InstanceBootstrap, Transport } from "@pidash/api-client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { InstanceAppGate } from "./InstanceAppGate.js";
import { instanceAppKeys } from "./queries.js";

function envelope(setupDone: boolean): InstanceBootstrap {
  return {
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
      is_setup_done: setupDone,
      is_signup_screen_visited: false,
      is_verified: false,
      is_test: false,
      is_current_version_deprecated: false,
      workspaces_exist: true,
    },
  };
}

function response(url: string, body: unknown): Awaited<ReturnType<Transport>> {
  return {
    status: 200,
    url,
    headers: { get: () => null },
    text: () => Promise.resolve(JSON.stringify(body)),
  };
}

function renderGate(transport: Transport, queryClient = new QueryClient()): void {
  createAppClient("https://api.test", transport);
  render(
    <QueryClientProvider client={queryClient}>
      <InstanceAppGate>
        <p>workspace content</p>
      </InstanceAppGate>
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

describe("InstanceAppGate", () => {
  it("shows a full-screen loader until the read settles", async () => {
    let release: (() => void) | undefined;
    renderGate(
      () =>
        new Promise((resolve) => {
          release = () => resolve(response("https://api.test/api/instances/", envelope(true)));
        })
    );
    expect(screen.getByTestId("instance-boot-loader")).toBeInTheDocument();
    expect(screen.queryByText("workspace content")).not.toBeInTheDocument();
    release?.();
    await waitFor(() => expect(screen.getByText("workspace content")).toBeInTheDocument());
  });

  it("replaces every route with maintenance when the read fails", async () => {
    renderGate(() =>
      Promise.resolve({
        status: 500,
        url: "https://api.test/api/instances/",
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "down" })),
      })
    );
    await waitFor(() => expect(screen.getByTestId("instance-maintenance")).toBeInTheDocument());
    expect(screen.queryByText("workspace content")).not.toBeInTheDocument();
  });

  it("shows the not-ready screen while setup is unfinished", async () => {
    renderGate(() => Promise.resolve(response("https://api.test/api/instances/", envelope(false))));
    await waitFor(() => expect(screen.getByTestId("instance-not-ready")).toBeInTheDocument());
    expect(screen.queryByText("workspace content")).not.toBeInTheDocument();
  });

  it("renders the app once setup is finished", async () => {
    renderGate(() => Promise.resolve(response("https://api.test/api/instances/", envelope(true))));
    await waitFor(() => expect(screen.getByText("workspace content")).toBeInTheDocument());
  });

  it("prefers a cached data-bearing answer over an error branch", () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(instanceAppKeys.bootstrap, envelope(true));
    renderGate(() => Promise.reject(new Error("must not refetch")), queryClient);
    expect(screen.getByText("workspace content")).toBeInTheDocument();
  });
});
