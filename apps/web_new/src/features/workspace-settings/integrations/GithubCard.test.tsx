// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// GitHub card (parity/specs/workspace-settings.md §16, WSET-017): the
// loading, read-failure, disconnected and connected forms; the Connect
// gate; a verified token clearing the field; a rejected token keeping it.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  GITHUB_CONNECTED_BADGE,
  GITHUB_CONNECTED_HINT,
  GITHUB_EXPLAINER,
  GITHUB_LOADING_STATUS,
  GITHUB_READ_FAILED_STATUS,
  GITHUB_RETRY_LABEL,
  GITHUB_TOKEN_LABEL,
  GITHUB_TOKEN_PLACEHOLDER,
  GithubCard,
} from "./GithubCard.js";

function renderCard(overrides: Partial<React.ComponentProps<typeof GithubCard>> = {}) {
  const props: React.ComponentProps<typeof GithubCard> = {
    status: { connected: false },
    statusPending: false,
    onConnect: vi.fn(async () => true),
    connectPending: false,
    onDisconnect: vi.fn(),
    disconnectPending: false,
    ...overrides,
  };
  return { ...render(<GithubCard {...props} />), props };
}

describe("GitHub card", () => {
  it("shows the loading state while the read is unresolved", () => {
    renderCard({ status: undefined, statusPending: true });
    expect(screen.getByRole("heading", { name: "GitHub" })).toBeInTheDocument();
    expect(screen.getByText(GITHUB_LOADING_STATUS)).toBeInTheDocument();
    expect(screen.queryByLabelText(GITHUB_TOKEN_LABEL)).not.toBeInTheDocument();
  });

  it("treats a resolved-but-missing status as loading", () => {
    renderCard({ status: undefined });
    expect(screen.getByText(GITHUB_LOADING_STATUS)).toBeInTheDocument();
  });

  it("offers a retry when the status read failed", async () => {
    const onRetry = vi.fn();
    renderCard({ status: undefined, readFailed: true, onRetry });
    expect(screen.getByText(GITHUB_READ_FAILED_STATUS)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: GITHUB_RETRY_LABEL }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("renders a retry without a handler for a failed read", async () => {
    renderCard({ status: undefined, readFailed: true });
    await userEvent.click(screen.getByRole("button", { name: GITHUB_RETRY_LABEL }));
    expect(screen.getByText(GITHUB_READ_FAILED_STATUS)).toBeInTheDocument();
  });

  it("keeps Connect disabled until the token holds something", async () => {
    const { props } = renderCard();
    const connect = screen.getByRole("button", { name: "Connect" });
    expect(connect).toBeDisabled();
    await userEvent.type(screen.getByLabelText(GITHUB_TOKEN_LABEL), "   ");
    expect(connect).toBeDisabled();
    await userEvent.type(screen.getByLabelText(GITHUB_TOKEN_LABEL), "pat");
    expect(connect).toBeEnabled();
    await userEvent.click(connect);
    expect(props.onConnect).toHaveBeenCalledWith("   pat");
  });

  it("clears the field only after the server verified the token", async () => {
    let resolve!: (value: boolean) => void;
    const onConnect = vi.fn(
      () =>
        new Promise<boolean>((done) => {
          resolve = done;
        })
    );
    renderCard({ onConnect });
    const field = screen.getByLabelText(GITHUB_TOKEN_LABEL);
    await userEvent.type(field, "github_pat_abc");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(onConnect).toHaveBeenCalledWith("github_pat_abc");
    expect(field).toHaveValue("github_pat_abc");
    resolve(true);
    await waitFor(() => expect(field).toHaveValue(""));
  });

  it("keeps the typed token when the server rejected it (NEWFRONT-421)", async () => {
    const onConnect = vi.fn(async () => false);
    renderCard({ onConnect });
    const field = screen.getByLabelText(GITHUB_TOKEN_LABEL);
    await userEvent.type(field, "github_pat_bad");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() => expect(onConnect).toHaveBeenCalled());
    expect(field).toHaveValue("github_pat_bad");
  });

  it("disables the form while a connect is in flight", () => {
    renderCard({ connectPending: true });
    expect(screen.getByLabelText(GITHUB_TOKEN_LABEL)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
  });

  it("renders the connected state with its login, hint and disconnect", async () => {
    const onDisconnect = vi.fn();
    renderCard({ status: { connected: true, github_user_login: "octo" }, onDisconnect });
    expect(screen.getByText("Connected as octo")).toBeInTheDocument();
    expect(screen.getByText(GITHUB_CONNECTED_HINT)).toBeInTheDocument();
    expect(screen.getByText(GITHUB_CONNECTED_BADGE)).toBeInTheDocument();
    expect(screen.queryByLabelText(GITHUB_TOKEN_LABEL)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(onDisconnect).toHaveBeenCalledTimes(1);
  });

  it("disables Disconnect while the disconnect is in flight", () => {
    renderCard({ status: { connected: true }, disconnectPending: true });
    expect(screen.getByRole("button", { name: "Disconnect" })).toBeDisabled();
  });

  it("names the disconnected explainer and help link on the form", () => {
    renderCard();
    expect(screen.getByText(GITHUB_EXPLAINER)).toBeInTheDocument();
    const link = screen.getByRole("link", { name: "Create a fine-grained token" });
    expect(link).toHaveAttribute("href", "https://github.com/settings/tokens?type=beta");
    expect(link).toHaveAttribute("target", "_blank");
    expect(screen.getByPlaceholderText(GITHUB_TOKEN_PLACEHOLDER)).toHaveAttribute("type", "password");
  });
});
