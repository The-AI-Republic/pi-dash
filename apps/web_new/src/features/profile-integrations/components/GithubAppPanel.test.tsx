// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One workspace's GitHub App card (parity/specs/profile.md §11, PROF-025):
// the lead line, badges, warning while the instance is unconfigured, the
// identifier grid, and the suspended and last-check banners.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { GithubAppWorkspace } from "@pidash/api-client";

import { CONFIG_MISSING_WARNING, SUSPENDED_BANNER, UNCONNECTED_COPY } from "../lib/copy.js";
import { GithubAppPanel } from "./GithubAppPanel.js";

const WORKSPACE_ID = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";

function workspace(overrides: Partial<GithubAppWorkspace["github_app"]> = {}): GithubAppWorkspace {
  return {
    id: WORKSPACE_ID,
    slug: "acme",
    name: "Acme",
    github_app: { connected: true, ...overrides },
  };
}

function renderPanel(overrides: Partial<React.ComponentProps<typeof GithubAppPanel>> = {}) {
  const props: React.ComponentProps<typeof GithubAppPanel> = {
    status: { configured: true, app_slug: "pi-dash", workspaces: [] },
    workspace: workspace(),
    onInstall: vi.fn(),
    onRefresh: vi.fn(),
    installing: false,
    refreshing: false,
    ...overrides,
  };
  render(<GithubAppPanel {...props} />);
  return props;
}

describe("GithubAppPanel", () => {
  it("names the account and workspace once connected", () => {
    renderPanel({ workspace: workspace({ account_login: "octo" }) });
    expect(screen.getByRole("heading", { name: "GitHub App" })).toBeInTheDocument();
    expect(screen.getByText("Installed for octo on Acme.")).toBeInTheDocument();
    expect(screen.getByText("Live")).toBeInTheDocument();
    expect(screen.queryByText(UNCONNECTED_COPY)).not.toBeInTheDocument();
  });

  it("falls back to the panel name when no account is named", () => {
    renderPanel();
    expect(screen.getByText("Installed for GitHub App on Acme.")).toBeInTheDocument();
  });

  it("invites an install before anything is connected", () => {
    renderPanel({ workspace: workspace({ connected: false }) });
    expect(screen.getByText("Install the Pi Dash GitHub App for Acme.")).toBeInTheDocument();
    expect(screen.getByText(UNCONNECTED_COPY)).toBeInTheDocument();
    expect(screen.queryByText("Live")).not.toBeInTheDocument();
    expect(screen.queryByTestId("github-app-status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Refresh" })).not.toBeInTheDocument();
  });

  it("offers reconnect and refresh once connected", async () => {
    const user = userEvent.setup();
    const props = renderPanel();
    await user.click(screen.getByRole("button", { name: "Reconnect" }));
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(props.onInstall).toHaveBeenCalledTimes(1);
    expect(props.onRefresh).toHaveBeenCalledTimes(1);
  });

  it("renders the identifier grid for a connected installation", () => {
    renderPanel({
      workspace: workspace({
        account_login: "octo",
        account_type: "Organization",
        repository_selection: "selected",
        repository_count: 3,
        verified_at: "2026-10-19T12:00:00Z",
        last_checked_at: "2026-10-19T13:00:00Z",
      }),
    });
    const grid = screen.getByTestId("github-app-status");
    expect(grid.firstElementChild?.children).toHaveLength(2);
    expect(within(grid).getByText("Account")).toBeInTheDocument();
    expect(within(grid).getByText("octo")).toBeInTheDocument();
    expect(within(grid).getByText("Selected repositories")).toBeInTheDocument();
    expect(within(grid).getByText("3")).toBeInTheDocument();
  });

  it("warns while the instance lacks its app configuration and disables install", () => {
    renderPanel({ status: { configured: false, app_slug: "", workspaces: [] } });
    expect(screen.getByRole("status")).toHaveTextContent(CONFIG_MISSING_WARNING);
    expect(screen.getByRole("button", { name: "Reconnect" })).toBeDisabled();
  });

  it("disables install while a handoff is in flight", () => {
    renderPanel({ installing: true });
    expect(screen.getByRole("button", { name: "Reconnect" })).toBeDisabled();
  });

  it("marks refresh busy while checking", () => {
    renderPanel({ refreshing: true });
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("banners a suspended installation without the live badge", () => {
    renderPanel({ workspace: workspace({ suspended_at: "2026-10-19T00:00:00Z" }) });
    expect(screen.getByRole("alert")).toHaveTextContent(SUSPENDED_BANNER);
    expect(screen.queryByText("Live")).not.toBeInTheDocument();
  });

  it("banners the host's last check failure", () => {
    renderPanel({ workspace: workspace({ last_check_error: "403 Forbidden" }) });
    expect(screen.getByRole("alert")).toHaveTextContent("Last check failed: 403 Forbidden");
  });
});
