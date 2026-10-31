// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import { AdminGeneralSettingsPage, GENERAL_SETTINGS_TITLE } from "./AdminGeneralSettingsPage.js";

function bootstrapBody(): Record<string, unknown> {
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
      id: "55555555-5555-4555-8555-555555555555",
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
      is_signup_screen_visited: true,
      is_verified: false,
      is_test: false,
      is_current_version_deprecated: false,
      workspaces_exist: true,
    },
  };
}

function adminsBody(): Record<string, unknown>[] {
  return [
    {
      id: "33333333-3333-4333-8333-333333333333",
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
      created_by: null,
      updated_by: null,
      deleted_at: null,
      user: "44444444-4444-4444-8444-444444444444",
      instance: "55555555-5555-4555-8555-555555555555",
      role: 1,
      is_verified: true,
      user_detail: {
        id: "44444444-4444-4444-8444-444444444444",
        first_name: "Ada",
        last_name: "Admin",
        avatar: "",
        avatar_url: null,
        is_bot: false,
        display_name: "Ada Admin",
        email: "ada@example.com",
        last_login_medium: "password",
      },
    },
  ];
}

interface PageFailures {
  bootstrap?: number;
  admins?: number;
}

function renderPage(failures: PageFailures = {}): void {
  const remaining = { bootstrap: failures.bootstrap ?? 0, admins: failures.admins ?? 0 };
  const transport: Transport = async (url: string) => {
    const path = new URL(url, "https://api.test").pathname;
    const route = (left: number, body: unknown) => ({
      status: left > 0 ? 500 : 200,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(left > 0 ? JSON.stringify({ detail: "boom" }) : JSON.stringify(body)),
    });
    if (path === "/api/instances/") {
      const response = route(remaining.bootstrap, bootstrapBody());
      remaining.bootstrap = Math.max(0, remaining.bootstrap - 1);
      return response;
    }
    if (path === "/api/instances/admins/") {
      const response = route(remaining.admins, adminsBody());
      remaining.admins = Math.max(0, remaining.admins - 1);
      return response;
    }
    return {
      status: 404,
      url,
      headers: { get: () => null },
      text: () => Promise.resolve(JSON.stringify({ detail: "not found" })),
    };
  };
  createAppClient("https://api.test", transport);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AdminGeneralSettingsPage />
    </QueryClientProvider>
  );
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("AdminGeneralSettingsPage", () => {
  it("loads the reads behind a loader, then renders the prefilled form", async () => {
    renderPage();
    expect(screen.getByRole("heading", { name: "General" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading general settings");
    expect(document.title).toBe(GENERAL_SETTINGS_TITLE);
    await waitFor(() => expect(screen.getByTestId("admin-general-form")).toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "Instance name" })).toHaveValue("Pi Dash");
    expect(screen.getByRole("textbox", { name: "Administrator email" })).toHaveValue("ada@example.com");
    expect(screen.getByRole("textbox", { name: "Instance ID" })).toHaveValue("instance-1");
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("retries both reads after a bootstrap failure", async () => {
    const user = userEvent.setup();
    renderPage({ bootstrap: 1 });
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("General settings failed to load."));
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByTestId("admin-general-form")).toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "Instance name" })).toHaveValue("Pi Dash");
  });

  it("retries both reads after an admins failure", async () => {
    const user = userEvent.setup();
    renderPage({ admins: 1 });
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("General settings failed to load."));
    await user.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(screen.getByTestId("admin-general-form")).toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "Administrator email" })).toHaveValue("ada@example.com");
  });
});
