// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { InstanceBootstrap, Transport } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { instanceAppKeys } from "../../instance/index.js";
import { AdminEntryGate, type AdminEntrySearch } from "./AdminEntryGate.js";

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

function stubTransport(seen: string[]): Transport {
  return (url) => {
    const path = new URL(url, "https://api.test").pathname;
    if (path === "/auth/get-csrf-token/") {
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-1" })),
      });
    }
    if (path === "/api/instances/") {
      seen.push(path);
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(envelope(true))),
      });
    }
    return new Promise(() => {});
  };
}

function renderGate(search: AdminEntrySearch = {}): { queryClient: QueryClient; seen: string[] } {
  const seen: string[] = [];
  createAppClient("https://api.test", stubTransport(seen));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AdminEntryGate search={search} />
    </QueryClientProvider>
  );
  return { queryClient, seen };
}

afterEach(() => {
  resetAppClient();
});

describe("AdminEntryGate", () => {
  it("shows only the loader while the read is unresolved", async () => {
    renderGate();
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expect(document.querySelector("form")).toBeNull();
    expect(screen.queryByTestId("admin-failure")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("admin-sign-in-form")).toBeInTheDocument());
  });

  it("resolves a set-up instance to the sign-in form from a single fetch", async () => {
    const { seen } = renderGate();
    await waitFor(() => expect(screen.getByTestId("admin-sign-in-form")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
    expect(seen).toHaveLength(1);
  });

  it("resolves an un-set-up instance to the setup form", async () => {
    const seen: string[] = [];
    createAppClient("https://api.test", ((url: string) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/auth/get-csrf-token/") {
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-1" })),
        });
      }
      seen.push(path);
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify(envelope(false))),
      });
    }) as Transport);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <AdminEntryGate search={{}} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("admin-setup-form")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
    expect(seen).toEqual(["/api/instances/"]);
  });

  it("resolves the bare unprovisioned envelope to the setup form", async () => {
    createAppClient("https://api.test", ((url: string) => {
      const path = new URL(url, "https://api.test").pathname;
      if (path === "/auth/get-csrf-token/") {
        return Promise.resolve({
          status: 200,
          url,
          headers: { get: () => null },
          text: () => Promise.resolve(JSON.stringify({ csrf_token: "csrf-1" })),
        });
      }
      return Promise.resolve({
        status: 200,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ is_activated: false, is_setup_done: false })),
      });
    }) as Transport);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <AdminEntryGate search={{}} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("admin-setup-form")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-failure")).not.toBeInTheDocument();
  });

  it("renders the failure screen when the read errors", async () => {
    createAppClient("https://api.test", ((url: string) =>
      Promise.resolve({
        status: 500,
        url,
        headers: { get: () => null },
        text: () => Promise.resolve(JSON.stringify({ detail: "boom" })),
      })) as Transport);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <AdminEntryGate search={{}} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("admin-failure")).toBeInTheDocument());
    expect(screen.queryByTestId("admin-loader")).not.toBeInTheDocument();
    expect(document.querySelector("form")).toBeNull();
  });

  it("forwards the redirect search to the resolved form", async () => {
    createAppClient("https://api.test", stubTransport([]));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(instanceAppKeys.bootstrap, { setupDone: false, bootstrap: envelope(false) });
    render(
      <QueryClientProvider client={queryClient}>
        <AdminEntryGate
          search={{
            first_name: "Parity",
            email: "a@example.com",
            company_name: "Prefill Co",
            error_code: "5180",
            error_message: "taken",
          }}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("admin-setup-form")).toBeInTheDocument();
    expect(screen.getByLabelText("First name")).toHaveValue("Parity");
    expect(screen.getByLabelText("Company")).toHaveValue("Prefill Co");
    expect(screen.getByTestId("admin-setup-banner")).toHaveTextContent("taken");
  });
});
