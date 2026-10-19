// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations page (parity/specs/workspace-settings.md §15–§17,
// WSET-016–018): the admin banner and both cards, the denial for members,
// the pending role, and the success/failure notices each provider write
// raises. A stub transport stands in for Django so the page runs through
// the real query and mutation hooks.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { permissions, notices } = vi.hoisted(() => ({
  permissions: {
    current: { isLoading: false, isAdmin: true, isMember: true, canEdit: true, role: 20 as number | null },
  },
  notices: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, usePermissions: () => permissions.current };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifySuccess: notices.success, notifyError: notices.error };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { SettingsNotAuthorized } from "../components/SettingsNotAuthorized.js";
import {
  GITHUB_CONNECTED_NOTICE,
  GITHUB_CONNECT_FAILED_NOTICE,
  GITHUB_DISCONNECTED_NOTICE,
  GITHUB_DISCONNECT_FAILED_NOTICE,
} from "./GithubCard.js";
import {
  GITLAB_CONNECTED_NOTICE,
  GITLAB_CONNECT_FAILED_NOTICE,
  GITLAB_DISCONNECTED_NOTICE,
  GITLAB_DISCONNECT_FAILED_NOTICE,
} from "./GitlabCard.js";
import { INTEGRATIONS_BANNER, INTEGRATIONS_HEADING, IntegrationsPage, IntegrationsScreen } from "./IntegrationsPage.js";

const SLUG = "acme";
const ACCOUNT = "123e4567-e89b-12d3-a456-426614174000";

interface Call {
  method: string;
  url: string;
  body: string | undefined;
}

function json(status: number, data: unknown): TransportResponse {
  return {
    status,
    url: "https://api.test/",
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(data),
  };
}

function accountRow(overrides: Record<string, unknown> = {}) {
  return {
    id: ACCOUNT,
    provider: "gitlab",
    host_url: "https://gitlab.example.com",
    auth_type: "pat",
    external_account_id: "7",
    external_account_login: "parity-gl",
    display_name: "Parity GitLab",
    capabilities: {},
    status: "connected",
    verified_at: "2026-10-01T00:00:00Z",
    last_check_error: "",
    ...overrides,
  };
}

/** A backend whose connect/register/disconnect answers can be switched per test. */
function backend(
  options: {
    github?: { connected: boolean; github_user_login?: string };
    accounts?: unknown[];
    connectGithub?: TransportResponse;
    disconnectGithub?: TransportResponse;
    register?: TransportResponse;
    disconnectAccount?: TransportResponse;
  } = {}
) {
  const calls: Call[] = [];
  let github = options.github ?? { connected: false };
  let accounts = options.accounts ?? [];
  const transport: Transport = async (url, init) => {
    calls.push({ method: init.method, url, body: init.body });
    const path = new URL(url).pathname;
    if (path === `/api/workspaces/${SLUG}/integrations/github/`) return json(200, github);
    if (path === `/api/workspaces/${SLUG}/integrations/github/connect/`) {
      if (options.connectGithub) return options.connectGithub;
      github = { connected: true, github_user_login: "octo" };
      return json(200, github);
    }
    if (path === `/api/workspaces/${SLUG}/integrations/github/disconnect/`) {
      if (options.disconnectGithub) return options.disconnectGithub;
      github = { connected: false };
      return json(200, github);
    }
    if (path === `/api/workspaces/${SLUG}/integrations/git/accounts/`) {
      if (init.method === "POST") {
        if (options.register) return options.register;
        accounts = [accountRow()];
        return json(201, accountRow());
      }
      return json(200, { accounts });
    }
    if (path === `/api/workspaces/${SLUG}/integrations/git/accounts/${ACCOUNT}/`) {
      if (options.disconnectAccount) return options.disconnectAccount;
      accounts = [accountRow({ status: "revoked" })];
      return json(200, { connected: false });
    }
    return json(404, { detail: "not found" });
  };
  return { transport, calls };
}

function renderPage() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <IntegrationsPage workspaceSlug={SLUG} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  permissions.current = { isLoading: false, isAdmin: true, isMember: true, canEdit: true, role: 20 };
});

afterEach(() => resetAppClient());

describe("integrations page", () => {
  it("shows the banner heading and both provider cards for an admin", async () => {
    createAppClient("https://api.test", backend().transport);
    renderPage();
    expect(screen.getByRole("heading", { name: INTEGRATIONS_HEADING })).toBeInTheDocument();
    expect(screen.getByText(INTEGRATIONS_BANNER)).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "GitHub" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "GitLab" })).toBeInTheDocument();
  });

  it("refuses a member with the shared not-authorized view and no cards", () => {
    permissions.current = { isLoading: false, isAdmin: false, isMember: true, canEdit: true, role: 15 };
    createAppClient("https://api.test", backend().transport);
    renderPage();
    expect(screen.getByText(SettingsNotAuthorized.name.length > 0 ? /not authorized/i : /x/)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument();
  });

  it("refuses a guest with no workspace role", () => {
    permissions.current = { isLoading: false, isAdmin: false, isMember: false, canEdit: false, role: null };
    createAppClient("https://api.test", backend().transport);
    renderPage();
    expect(screen.getByRole("heading", { name: /not authorized/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument();
  });

  it("shows the pending state while the role resolves", () => {
    permissions.current = { isLoading: true, isAdmin: false, isMember: false, canEdit: false, role: null };
    createAppClient("https://api.test", backend().transport);
    renderPage();
    expect(screen.getByRole("heading", { name: INTEGRATIONS_HEADING })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitLab" })).not.toBeInTheDocument();
    expect(screen.queryByText(/not authorized/i)).not.toBeInTheDocument();
  });

  it("shows a retry when a provider read fails", async () => {
    const failing: Transport = async () => json(500, { detail: "boom" });
    createAppClient("https://api.test", failing);
    renderPage();
    expect(await screen.findByText("We could not load the GitHub connection.")).toBeInTheDocument();
    expect(await screen.findByText("We could not load the GitLab accounts.")).toBeInTheDocument();
    await userEvent.click(screen.getAllByRole("button", { name: "Try again" })[0]!);
    expect(screen.getAllByRole("button", { name: "Try again" })).toHaveLength(2);
  });

  it("re-reads the GitLab accounts when its retry runs", async () => {
    let accountsReads = 0;
    const transport: Transport = async (url) => {
      const path = new URL(url).pathname;
      if (path === `/api/workspaces/${SLUG}/integrations/github/`) return json(200, { connected: false });
      if (path === `/api/workspaces/${SLUG}/integrations/git/accounts/`) {
        accountsReads += 1;
        return accountsReads === 1 ? json(500, { detail: "boom" }) : json(200, { accounts: [] });
      }
      return json(404, { detail: "not found" });
    };
    createAppClient("https://api.test", transport);
    renderPage();
    expect(await screen.findByText("We could not load the GitLab accounts.")).toBeInTheDocument();
    const gitlabCard = screen.getByTestId("integration-card-gitlab");
    await userEvent.click(within(gitlabCard).getByRole("button", { name: "Try again" }));
    expect(
      await screen.findByText(
        "Connect a GitLab token with API access to bind repositories and mirror GitLab issues into your projects."
      )
    ).toBeInTheDocument();
    expect(accountsReads).toBeGreaterThan(1);
  });

  it("connects GitHub with a notice, clears the field and re-reads the status", async () => {
    const api = backend();
    createAppClient("https://api.test", api.transport);
    renderPage();
    const field = await screen.findByLabelText("Personal access token");
    await userEvent.type(field, "github_pat_abc");
    await userEvent.click(
      within(screen.getByTestId("integration-card-github")).getByRole("button", { name: "Connect" })
    );
    await waitFor(() => expect(notices.success).toHaveBeenCalledWith(GITHUB_CONNECTED_NOTICE, expect.anything()));
    expect(await screen.findByText("Connected as octo")).toBeInTheDocument();
    expect(api.calls.find((call) => call.url.includes("/github/connect/"))?.body).toBe(
      JSON.stringify({ token: "github_pat_abc" })
    );
  });

  it("keeps the token and shows the failure notice for a rejected GitHub credential", async () => {
    const api = backend({ connectGithub: json(401, { detail: "bad token" }) });
    createAppClient("https://api.test", api.transport);
    renderPage();
    const field = await screen.findByLabelText("Personal access token");
    await userEvent.type(field, "github_pat_bad");
    await userEvent.click(
      within(screen.getByTestId("integration-card-github")).getByRole("button", { name: "Connect" })
    );
    await waitFor(() => expect(notices.error).toHaveBeenCalledWith(GITHUB_CONNECT_FAILED_NOTICE, expect.anything()));
    expect(field).toHaveValue("github_pat_bad");
    expect(screen.queryByText(/Connected as/)).not.toBeInTheDocument();
  });

  it("disconnects GitHub with a notice", async () => {
    createAppClient("https://api.test", backend({ github: { connected: true, github_user_login: "octo" } }).transport);
    renderPage();
    expect(await screen.findByText("Connected as octo")).toBeInTheDocument();
    await userEvent.click(
      within(screen.getByTestId("integration-card-github")).getByRole("button", { name: "Disconnect" })
    );
    await waitFor(() => expect(notices.success).toHaveBeenCalledWith(GITHUB_DISCONNECTED_NOTICE, expect.anything()));
    expect(await screen.findByLabelText("Personal access token")).toBeInTheDocument();
  });

  it("shows the error notice when the GitHub disconnect fails", async () => {
    const api = backend({
      github: { connected: true, github_user_login: "octo" },
      disconnectGithub: json(500, { detail: "boom" }),
    });
    createAppClient("https://api.test", api.transport);
    renderPage();
    expect(await screen.findByText("Connected as octo")).toBeInTheDocument();
    await userEvent.click(
      within(screen.getByTestId("integration-card-github")).getByRole("button", { name: "Disconnect" })
    );
    await waitFor(() => expect(notices.error).toHaveBeenCalledWith(GITHUB_DISCONNECT_FAILED_NOTICE, expect.anything()));
    expect(screen.getByText("Connected as octo")).toBeInTheDocument();
  });

  it("registers a GitLab account with a notice and clears the token", async () => {
    const api = backend();
    createAppClient("https://api.test", api.transport);
    renderPage();
    const token = await screen.findByLabelText("Token");
    await userEvent.type(screen.getByLabelText("GitLab host (optional)"), "https://gitlab.example.com");
    await userEvent.type(token, "glpat-abc");
    await userEvent.click(
      within(screen.getByTestId("integration-card-gitlab")).getByRole("button", { name: "Connect" })
    );
    await waitFor(() => expect(notices.success).toHaveBeenCalledWith(GITLAB_CONNECTED_NOTICE, expect.anything()));
    expect(await screen.findByText("Connected as parity-gl on https://gitlab.example.com")).toBeInTheDocument();
    expect(api.calls.find((call) => call.method === "POST")?.body).toBe(
      JSON.stringify({
        provider: "gitlab",
        token: "glpat-abc",
        auth_type: "pat",
        host_url: "https://gitlab.example.com",
      })
    );
  });

  it("keeps the token and shows the failure notice for a rejected GitLab credential", async () => {
    const api = backend({ register: json(401, { detail: "bad token" }) });
    createAppClient("https://api.test", api.transport);
    renderPage();
    const token = await screen.findByLabelText("Token");
    await userEvent.type(token, "glpat-bad");
    await userEvent.click(
      within(screen.getByTestId("integration-card-gitlab")).getByRole("button", { name: "Connect" })
    );
    await waitFor(() => expect(notices.error).toHaveBeenCalledWith(GITLAB_CONNECT_FAILED_NOTICE, expect.anything()));
    expect(token).toHaveValue("glpat-bad");
  });

  it("disconnects the connected GitLab account individually with a notice", async () => {
    const api = backend({ accounts: [accountRow()] });
    createAppClient("https://api.test", api.transport);
    renderPage();
    expect(await screen.findByText("Connected as parity-gl on https://gitlab.example.com")).toBeInTheDocument();
    await userEvent.click(
      within(screen.getByTestId("integration-card-gitlab")).getByRole("button", { name: "Disconnect" })
    );
    await waitFor(() => expect(notices.success).toHaveBeenCalledWith(GITLAB_DISCONNECTED_NOTICE, expect.anything()));
    expect(await screen.findByLabelText("Token")).toBeInTheDocument();
    expect(api.calls.some((call) => call.method === "DELETE" && call.url.endsWith(`/accounts/${ACCOUNT}/`))).toBe(true);
  });

  it("shows the error notice when a GitLab disconnect fails", async () => {
    const api = backend({ accounts: [accountRow()], disconnectAccount: json(500, { detail: "boom" }) });
    createAppClient("https://api.test", api.transport);
    renderPage();
    expect(await screen.findByText("Connected as parity-gl on https://gitlab.example.com")).toBeInTheDocument();
    await userEvent.click(
      within(screen.getByTestId("integration-card-gitlab")).getByRole("button", { name: "Disconnect" })
    );
    await waitFor(() => expect(notices.error).toHaveBeenCalledWith(GITLAB_DISCONNECT_FAILED_NOTICE, expect.anything()));
  });

  it("renders the screen body for a direct mount", async () => {
    createAppClient("https://api.test", backend().transport);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <IntegrationsScreen workspaceSlug={SLUG} />
      </QueryClientProvider>
    );
    expect(await screen.findByLabelText("Personal access token")).toBeInTheDocument();
  });
});
