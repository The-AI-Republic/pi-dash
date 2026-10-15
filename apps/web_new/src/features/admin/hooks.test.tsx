// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { useAdminCsrfToken, useInstanceBootstrap } from "./hooks.js";

function stubTransport(routes: Record<string, { status: number; body: unknown }>): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    const route = routes[path] ?? { status: 404, body: { detail: "not found" } };
    return Promise.resolve({
      status: route.status,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify(route.body)),
    });
  };
}

function renderProbe(Probe: () => React.ReactElement): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Probe />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
});

function BootstrapProbe(): React.ReactElement {
  const query = useInstanceBootstrap();
  if (query.isPending) return <div data-testid="probe">loading</div>;
  if (query.isError) return <div data-testid="probe">failed</div>;
  return <div data-testid="probe">{query.data.instance.is_setup_done ? "set-up" : "first-run"}</div>;
}

describe("useInstanceBootstrap", () => {
  const envelope = (setupDone: boolean) => ({
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
  });

  it("reads the setup flag through the app client", async () => {
    createAppClient("https://api.test", stubTransport({ "/api/instances/": { status: 200, body: envelope(true) } }));
    renderProbe(BootstrapProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("set-up"));
  });

  it("reports the first-run branch", async () => {
    createAppClient("https://api.test", stubTransport({ "/api/instances/": { status: 200, body: envelope(false) } }));
    renderProbe(BootstrapProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("first-run"));
  });

  it("surfaces fetch failures instead of retrying", async () => {
    createAppClient("https://api.test", stubTransport({}));
    renderProbe(BootstrapProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("failed"));
  });
});

function CsrfProbe(): React.ReactElement {
  const query = useAdminCsrfToken();
  return <div data-testid="probe">{query.data === undefined ? "loading" : query.data.csrf_token}</div>;
}

describe("useAdminCsrfToken", () => {
  it("reads the forgery token through the app client", async () => {
    createAppClient(
      "https://api.test",
      stubTransport({ "/auth/get-csrf-token/": { status: 200, body: { csrf_token: "token-1" } } })
    );
    renderProbe(CsrfProbe);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("token-1"));
  });
});
