// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ToastHost } from "@pidash/kit";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import { expiryLine, TOKEN_DELETED_NOTICE } from "./model.js";
import { PersonalAccessTokensPage } from "./PersonalAccessTokensPage.js";

const TOKEN_ID = "123e4567-e89b-12d3-a456-426614174000";
const USER_ID = "223e4567-e89b-12d3-a456-426614174001";

function token(overrides: Record<string, unknown> = {}) {
  return {
    id: TOKEN_ID,
    label: "Deploy bot",
    description: "Nightly deploy",
    is_active: true,
    last_used: null,
    user: USER_ID,
    user_type: 0,
    workspace: null,
    expired_at: null,
    is_service: false,
    allowed_rate_limit: "60/minute",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: USER_ID,
    updated_by: USER_ID,
    ...overrides,
  };
}

function json(url: string, body: string, status = 200) {
  return { status, url, headers: { get: () => "application/json" }, text: async () => body };
}

function harness(handler: (url: string, method: string) => Promise<ReturnType<typeof json>> | ReturnType<typeof json>) {
  createAppClient("https://api.test", (async (url: string, init: { method: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
    return handler(url, init.method);
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
      <ToastHost manager={getToastManager()} />
    </QueryClientProvider>
  );
  return { queryClient, wrapper };
}

afterEach(() => {
  resetAppClient();
});

describe("PersonalAccessTokensPage", () => {
  it("shows the loader while the list resolves", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { wrapper } = harness(async (url) => {
      await gate;
      return json(url, "[]");
    });
    render(<PersonalAccessTokensPage />, { wrapper });
    expect(screen.getByTestId("api-tokens-loader")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add personal access token" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add API token" })).toBeNull();
    await act(async () => release?.());
    await screen.findByText("No Personal token yet");
  });

  it("shows the illustrated empty state and opens the create dialog from both actions", async () => {
    const user = userEvent.setup();
    const { wrapper } = harness((url) => json(url, "[]"));
    render(<PersonalAccessTokensPage />, { wrapper });
    await screen.findByText("No Personal token yet");
    expect(screen.getByRole("heading", { name: "Personal Access Tokens", level: 1 })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add API token" }));
    expect(screen.getByRole("dialog", { name: "Generate a personal access token" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Add personal access token" }));
    expect(screen.getByRole("dialog", { name: "Generate a personal access token" })).toBeInTheDocument();
  });

  it("renders the load-error branch with no empty-state action", async () => {
    const { wrapper } = harness((url) => json(url, '{"detail":"boom"}', 500));
    render(<PersonalAccessTokensPage />, { wrapper });
    expect(await screen.findByRole("alert")).toHaveTextContent("Personal access tokens could not be loaded.");
    expect(screen.queryByRole("button", { name: "Add API token" })).toBeNull();
  });

  it("renders rows with liveness, description and the three expiry wordings", async () => {
    const tokens = [
      token({ id: "11111111-1111-4111-8111-111111111111", label: "Future", expired_at: "2030-06-15T12:00:00Z" }),
      token({ id: "22222222-2222-4222-8222-222222222222", label: "Never", description: "  " }),
      token({
        id: "33333333-3333-4333-8333-333333333333",
        label: "Expired",
        is_active: false,
        expired_at: "2020-01-01T00:00:00Z",
      }),
    ];
    const { wrapper } = harness((url) => json(url, JSON.stringify(tokens)));
    render(<PersonalAccessTokensPage now={new Date("2026-10-19T12:00:00Z")} />, { wrapper });
    expect(await screen.findByRole("heading", { name: "Future", level: 5 })).toBeInTheDocument();
    const future = screen.getByRole("heading", { name: "Future", level: 5 }).closest("li") as HTMLElement;
    expect(future).toHaveTextContent("Active");
    expect(future).toHaveTextContent(expiryLine("2030-06-15T12:00:00Z"));
    const never = screen.getByRole("heading", { name: "Never", level: 5 }).closest("li") as HTMLElement;
    expect(never).toHaveTextContent("Never expires");
    expect(never.querySelectorAll("p")).toHaveLength(1);
    const expired = screen.getByRole("heading", { name: "Expired", level: 5 }).closest("li") as HTMLElement;
    expect(expired).toHaveTextContent("Expired");
    expect(expired).toHaveTextContent(/^.*Expired \d+ years ago.*$/s);
    expect(screen.queryByText("No Personal token yet")).toBeNull();
  });

  it("renders row expiry lines in the member timezone", async () => {
    const tokens = [
      token({ id: "11111111-1111-4111-8111-111111111111", label: "Future", expired_at: "2030-06-15T12:00:00Z" }),
    ];
    const me = {
      id: USER_ID,
      email: "member@example.com",
      username: "member",
      first_name: "Member",
      last_name: "User",
      display_name: "Member User",
      avatar: "",
      avatar_url: null,
      cover_image: null,
      cover_image_url: null,
      date_joined: "2026-01-01T00:00:00Z",
      user_timezone: "Pacific/Kiritimati",
      is_active: true,
      is_bot: false,
      is_email_verified: true,
      is_password_autoset: false,
      last_login_medium: "password",
      last_login_time: null,
    };
    const { wrapper } = harness((url) =>
      url.endsWith("/api/users/me/") ? json(url, JSON.stringify(me)) : json(url, JSON.stringify(tokens))
    );
    render(<PersonalAccessTokensPage now={new Date("2026-10-19T12:00:00Z")} />, { wrapper });
    expect(await screen.findByRole("heading", { name: "Future", level: 5 })).toBeInTheDocument();
    const future = screen.getByRole("heading", { name: "Future", level: 5 }).closest("li") as HTMLElement;
    expect(future).toHaveTextContent("Expires Jun 16, 2030 at 02:00");
  });

  it("confirms deletion, cancels without a request, and deletes with a notice", async () => {
    const user = userEvent.setup();
    const deletes: string[] = [];
    const { wrapper } = harness((url, method) => {
      if (method === "GET") return json(url, JSON.stringify([token()]));
      deletes.push(url);
      return json(url, "", 204);
    });
    render(<PersonalAccessTokensPage />, { wrapper });
    await screen.findByRole("heading", { name: "Deploy bot", level: 5 });
    await user.click(screen.getByRole("button", { name: "Delete Deploy bot" }));
    const dialog = screen.getByRole("dialog", { name: "Delete personal access token" });
    expect(dialog).toHaveTextContent(/cannot be undone/);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(deletes).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Delete Deploy bot" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Deploy bot", level: 5 })).toBeNull());
    expect(deletes).toEqual([`https://api.test/api/users/api-tokens/${TOKEN_ID}`]);
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent(TOKEN_DELETED_NOTICE));
    expect(await screen.findByText("No Personal token yet")).toBeInTheDocument();
  });

  it("keeps the row and reports when deletion fails", async () => {
    const user = userEvent.setup();
    const { wrapper } = harness((url, method) => {
      if (method === "GET") return json(url, JSON.stringify([token()]));
      return json(url, '{"detail":"boom"}', 500);
    });
    render(<PersonalAccessTokensPage />, { wrapper });
    await screen.findByRole("heading", { name: "Deploy bot", level: 5 });
    await user.click(screen.getByRole("button", { name: "Delete Deploy bot" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(screen.getByTestId("toast-stack")).toHaveTextContent("Could not delete the personal access token")
    );
    expect(screen.getByRole("heading", { name: "Deploy bot", level: 5 })).toBeInTheDocument();
  });

  it("closes the delete confirmation from the dialog's dismiss control and its backdrop", async () => {
    const user = userEvent.setup();
    const { wrapper } = harness((url) => json(url, JSON.stringify([token()])));
    render(<PersonalAccessTokensPage />, { wrapper });
    await screen.findByRole("heading", { name: "Deploy bot", level: 5 });
    await user.click(screen.getByRole("button", { name: "Delete Deploy bot" }));
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Delete Deploy bot" }));
    await user.click(document.querySelector("[data-base-ui-backdrop]") ?? document.body);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
