// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// GitLab card (parity/specs/workspace-settings.md §17, WSET-018): loading,
// read failure, disconnected form (optional host), the connected account
// naming login and host, and per-account disconnect.
import type { GitProviderAccount } from "@pidash/api-client";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  GITLAB_CONNECTED_BADGE,
  GITLAB_CONNECTED_HINT,
  GITLAB_EXPLAINER,
  GITLAB_HOST_LABEL,
  GITLAB_HOST_PLACEHOLDER,
  GITLAB_LOADING_STATUS,
  GITLAB_READ_FAILED_STATUS,
  GITLAB_RETRY_LABEL,
  GITLAB_TOKEN_LABEL,
  GITLAB_TOKEN_PLACEHOLDER,
  GitlabCard,
  gitlabConnectedStatus,
} from "./GitlabCard.js";

function account(overrides: Partial<GitProviderAccount> = {}): GitProviderAccount {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
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

function renderCard(overrides: Partial<React.ComponentProps<typeof GitlabCard>> = {}) {
  const props: React.ComponentProps<typeof GitlabCard> = {
    accounts: [],
    accountsPending: false,
    onConnect: vi.fn(async () => true),
    connectPending: false,
    onDisconnect: vi.fn(),
    disconnectPending: false,
    ...overrides,
  };
  return { ...render(<GitlabCard {...props} />), props };
}

describe("GitLab card", () => {
  it("shows the loading state while the accounts read is unresolved", () => {
    renderCard({ accounts: undefined, accountsPending: true });
    expect(screen.getByRole("heading", { name: "GitLab" })).toBeInTheDocument();
    expect(screen.getByText(GITLAB_LOADING_STATUS)).toBeInTheDocument();
  });

  it("treats a resolved-but-missing list as loading", () => {
    renderCard({ accounts: undefined });
    expect(screen.getByText(GITLAB_LOADING_STATUS)).toBeInTheDocument();
  });

  it("offers a retry when the accounts read failed", async () => {
    const onRetry = vi.fn();
    renderCard({ accounts: undefined, readFailed: true, onRetry });
    expect(screen.getByText(GITLAB_READ_FAILED_STATUS)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: GITLAB_RETRY_LABEL }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("renders a retry without a handler for a failed read", async () => {
    renderCard({ accounts: undefined, readFailed: true });
    await userEvent.click(screen.getByRole("button", { name: GITLAB_RETRY_LABEL }));
    expect(screen.getByText(GITLAB_READ_FAILED_STATUS)).toBeInTheDocument();
  });

  it("shows the form when every account is revoked", () => {
    renderCard({ accounts: [account({ status: "revoked" })] });
    expect(screen.getByText(GITLAB_EXPLAINER)).toBeInTheDocument();
    expect(screen.getByLabelText(GITLAB_HOST_LABEL)).toHaveValue("");
    expect(screen.getByLabelText(GITLAB_TOKEN_LABEL)).toHaveValue("");
    expect(screen.getByPlaceholderText(GITLAB_HOST_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(GITLAB_TOKEN_PLACEHOLDER)).toBeInTheDocument();
  });

  it("gates Connect on the token and sends the trimmed values", async () => {
    const { props } = renderCard();
    const connect = screen.getByRole("button", { name: "Connect" });
    expect(connect).toBeDisabled();
    await userEvent.type(screen.getByLabelText(GITLAB_HOST_LABEL), "  https://git.acme.test ");
    expect(connect).toBeDisabled();
    await userEvent.type(screen.getByLabelText(GITLAB_TOKEN_LABEL), " glpat-abc ");
    expect(connect).toBeEnabled();
    await userEvent.click(connect);
    expect(props.onConnect).toHaveBeenCalledWith({ token: "glpat-abc", hostUrl: "https://git.acme.test" });
  });

  it("clears the token only after the account registered", async () => {
    let resolve!: (value: boolean) => void;
    const onConnect = vi.fn(
      () =>
        new Promise<boolean>((done) => {
          resolve = done;
        })
    );
    renderCard({ onConnect });
    const field = screen.getByLabelText(GITLAB_TOKEN_LABEL);
    await userEvent.type(field, "glpat-abc");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(field).toHaveValue("glpat-abc");
    resolve(true);
    await waitFor(() => expect(field).toHaveValue(""));
  });

  it("keeps the typed token when the provider rejected it (NEWFRONT-421)", async () => {
    const onConnect = vi.fn(async () => false);
    renderCard({ onConnect });
    const field = screen.getByLabelText(GITLAB_TOKEN_LABEL);
    await userEvent.type(field, "glpat-bad");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(onConnect).toHaveBeenCalled());
    expect(field).toHaveValue("glpat-bad");
  });

  it("disables the form while a registration is in flight", () => {
    renderCard({ connectPending: true });
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
  });

  it("names the connected login and host and disconnects that account", async () => {
    const onDisconnect = vi.fn();
    renderCard({ accounts: [account()], onDisconnect });
    expect(screen.getByText("Connected as parity-gl on https://gitlab.example.com")).toBeInTheDocument();
    expect(screen.getByText(GITLAB_CONNECTED_HINT)).toBeInTheDocument();
    expect(screen.getByText(GITLAB_CONNECTED_BADGE)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(onDisconnect).toHaveBeenCalledWith("123e4567-e89b-12d3-a456-426614174000");
  });

  it("disables Disconnect while a disconnect is in flight", () => {
    renderCard({ accounts: [account()], disconnectPending: true });
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeDisabled();
  });

  it("builds the status from the identity fallbacks", () => {
    expect(gitlabConnectedStatus(account())).toBe("Connected as parity-gl on https://gitlab.example.com");
    expect(gitlabConnectedStatus(account({ external_account_login: "", display_name: "Name" }))).toBe(
      "Connected as Name on https://gitlab.example.com"
    );
    expect(gitlabConnectedStatus(account({ external_account_login: "", display_name: "" }))).toBe(
      "Connected as GitLab on https://gitlab.example.com"
    );
  });
});
