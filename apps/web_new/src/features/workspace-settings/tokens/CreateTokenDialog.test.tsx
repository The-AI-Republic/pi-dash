// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ToastHost } from "@pidash/kit";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import { CreateTokenDialog } from "./CreateTokenDialog.js";
import { accessibleDayName } from "./TokenExpiryPicker.js";
import {
  emptyTokenDraft,
  EXPIRY_REQUIRED_NOTICE,
  expiryLine,
  resolveExpiry,
  revealExpiryLine,
  TOKEN_COPIED_NOTICE,
  TOKEN_BACKUP_FAILED_NOTICE,
  TITLE_REQUIRED_ERROR,
  TITLE_TOO_LONG_ERROR,
} from "./model.js";

const NOW = new Date(2026, 9, 19, 11, 30, 0, 0);
const TOKEN_ID = "123e4567-e89b-12d3-a456-426614174000";
const USER_ID = "223e4567-e89b-12d3-a456-426614174001";

function createdToken(overrides: Record<string, unknown> = {}) {
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
    token: "pi_dash_api_abc123",
    ...overrides,
  };
}

function json(url: string, body: string, status = 200) {
  return { status, url, headers: { get: () => "application/json" }, text: async () => body };
}

interface CreatedCall {
  url: string;
  method: string;
  body: string;
}

function harness(reply: (url: string, method: string) => ReturnType<typeof json> | Promise<ReturnType<typeof json>>) {
  const calls: CreatedCall[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
    calls.push({ url, method: init.method, body: init.body ?? "" });
    return reply(url, init.method);
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <React.Fragment>{children}</React.Fragment>
      <ToastHost manager={getToastManager()} />
    </QueryClientProvider>
  );
  return { calls, queryClient, wrapper };
}

/** jsdom's Blob has no text(); FileReader is present in the test DOM. */
function readBlob(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(blob);
  });
}

/**
 * No per-keystroke delay: userEvent's default schedules a timer between
 * every character, so typing the long title below spent seconds of real
 * time. A test that runs out of budget mid-typing used to leave its queued
 * keystrokes to land in the next test's fields.
 */
function typingUser() {
  return userEvent.setup({ delay: null });
}

/**
 * Over-long titles go in as one value rather than 256 awaited keystrokes.
 * The control is a plain input with no paste handler or maxlength, so this
 * reaches the same onChange path; a maxlength would truncate the value and
 * fail the validation assertion below.
 */
async function enterTitle(user: ReturnType<typeof typingUser>, value: string): Promise<void> {
  const title = screen.getByPlaceholderText("Title");
  await user.click(title);
  await user.paste(value);
}

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("CreateTokenDialog validation", () => {
  it("closes on Escape before the reveal", async () => {
    const onClose = vi.fn();
    const { wrapper } = harness((url) => json(url, "{}"));
    const user = typingUser();
    render(<CreateTokenDialog open onClose={onClose} now={NOW} />, { wrapper });
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("rejects an empty title inline without creating", async () => {
    const { calls, wrapper } = harness((url) => json(url, "{}"));
    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    expect(await screen.findByText(TITLE_REQUIRED_ERROR)).toBeInTheDocument();
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
  });

  it("rejects blank and over-long titles inline", async () => {
    const { wrapper } = harness((url) => json(url, "{}"));
    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await enterTitle(user, "   ");
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    expect(await screen.findByText(TITLE_REQUIRED_ERROR)).toBeInTheDocument();
    await user.clear(screen.getByPlaceholderText("Title"));
    await enterTitle(user, "x".repeat(256));
    expect(screen.getByPlaceholderText("Title")).toHaveValue("x".repeat(256));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    expect(await screen.findByText(TITLE_TOO_LONG_ERROR)).toBeInTheDocument();
  });

  it("shows the missing-expiry notice and keeps the dialog open", async () => {
    const { calls, wrapper } = harness((url) => json(url, "{}"));
    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent(EXPIRY_REQUIRED_NOTICE));
    expect(screen.getByRole("dialog", { name: "Generate a personal access token" })).toBeInTheDocument();
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
  });

  it("cancels without closing through the create request", async () => {
    const onClose = vi.fn();
    const { calls, wrapper } = harness((url) => json(url, "{}"));
    const user = typingUser();
    render(<CreateTokenDialog open onClose={onClose} now={NOW} />, { wrapper });
    await user.click(screen.getByText("Cancel"));
    expect(onClose).toHaveBeenCalledOnce();
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
  });
});

describe("CreateTokenDialog create and reveal", () => {
  it("creates with the label, description and custom expiry, then reveals once with copy and backup", async () => {
    const created = createdToken({ expired_at: "2026-11-15T18:30:00.000Z" });
    const { calls, wrapper } = harness((url) => json(url, JSON.stringify(created), 201));
    const createObjectURL = vi.fn((_blob: Blob) => "blob:token");
    const revokeObjectURL = vi.fn();
    Object.assign(URL, { createObjectURL, revokeObjectURL });
    const anchorClick = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);

    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.type(screen.getByPlaceholderText("Description"), "Nightly deploy");
    const dialog = screen.getByRole("dialog", { name: "Generate a personal access token" });
    await user.click(within(dialog).getAllByRole("button")[0] as HTMLElement);
    await user.click(screen.getByRole("option", { name: "Custom" }));
    await user.click(screen.getByText("Set date"));
    await user.click(screen.getByRole("button", { name: "Go to the Next Month" }));
    await user.click(screen.getByRole("button", { name: accessibleDayName("2026-11-15") }));
    const expectedExpiry = resolveExpiry(
      { ...emptyTokenDraft(), choice: "custom", customDay: "2026-11-15" },
      NOW
    ) as string;
    expect(screen.getByText(expiryLine(expectedExpiry))).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Generate token" }));

    expect(await screen.findByRole("heading", { name: "Key created" })).toBeInTheDocument();
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toBe("https://api.test/api/users/api-tokens/");
    expect(JSON.parse(post?.body ?? "{}")).toEqual({
      label: "Deploy bot",
      description: "Nightly deploy",
      expired_at: expectedExpiry,
    });
    expect(screen.getByTestId("token-secret").textContent).toBe("pi_dash_api_abc123");
    expect(screen.getByText(revealExpiryLine(created.expired_at))).toBeInTheDocument();
    expect(createObjectURL).toHaveBeenCalledOnce();
    expect(anchorClick).toHaveBeenCalledOnce();
    const blob = createObjectURL.mock.calls[0]?.[0] as Blob;
    const csv = await readBlob(blob);
    expect(csv).toContain("Title,Description,Expiry,Secret key");
    expect(csv).toContain("Deploy bot");
    expect(csv).toContain("pi_dash_api_abc123");

    const reveal = screen.getByRole("dialog", { name: "Key created" });
    await user.click(within(reveal).getAllByRole("button")[0] as HTMLElement);
    expect(await screen.findByTestId("toast-stack")).toHaveTextContent(TOKEN_COPIED_NOTICE);
    expect(screen.getByRole("button", { name: "Token copied" })).toBeInTheDocument();
  });

  it("creates with no expiry, backs up Never expires, and closes without leaking the secret", async () => {
    const created = createdToken({ expired_at: null });
    const { calls, wrapper } = harness((url) => json(url, JSON.stringify(created), 201));
    const createObjectURL = vi.fn((_blob: Blob) => "blob:never");
    Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);

    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await screen.findByRole("heading", { name: "Key created" });
    expect(JSON.parse(calls.find((call) => call.method === "POST")?.body ?? "{}")).toMatchObject({ expired_at: null });
    expect(screen.getByText("Never expires")).toBeInTheDocument();
    const blob = createObjectURL.mock.calls[0]?.[0] as Blob;
    expect(await readBlob(blob)).toContain("Never expires");
    await user.click(screen.getByText("Close"));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Key created" })).toBeNull());
    expect(screen.queryByText("pi_dash_api_abc123")).toBeNull();
  });

  it("keeps the form and reports a failed create", async () => {
    const { calls, wrapper } = harness((url) => json(url, '{"detail":"Label is required"}', 400));
    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("button", { name: "Set expiration" }));
    await user.click(screen.getByRole("option", { name: "1 month" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await waitFor(() =>
      expect(screen.getByTestId("toast-stack")).toHaveTextContent("Could not create the personal access token")
    );
    expect(screen.getByRole("dialog", { name: "Generate a personal access token" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Deploy bot");
    expect(calls.filter((call) => call.method === "POST")).toHaveLength(1);
  });

  it("treats a copy failure as a notice without leaving the reveal", async () => {
    const created = createdToken();
    const { wrapper } = harness((url) => json(url, JSON.stringify(created), 201));
    Object.assign(URL, { createObjectURL: vi.fn(() => "blob:fail"), revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const writeText = vi.fn(() => Promise.reject(new Error("denied")));
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });

    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    fireEvent.change(screen.getByPlaceholderText("Title"), { target: { value: "Deploy bot" } });
    fireEvent.click(screen.getByRole("switch", { name: "Never expires" }));
    fireEvent.click(screen.getByRole("button", { name: "Generate token" }));
    const reveal = await screen.findByRole("dialog", { name: "Key created" });
    fireEvent.click(within(reveal).getAllByRole("button")[0] as HTMLElement);
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent("Could not copy the token"));
    expect(screen.getByRole("heading", { name: "Key created" })).toBeInTheDocument();
  });

  it("keeps the reveal and reports when the backup download is refused", async () => {
    const created = createdToken();
    const { wrapper } = harness((url) => json(url, JSON.stringify(created), 201));
    Object.assign(URL, {
      createObjectURL: vi.fn(() => {
        throw new Error("blocked");
      }),
      revokeObjectURL: vi.fn(),
    });
    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await screen.findByRole("heading", { name: "Key created" });
    expect(screen.getByTestId("token-secret")).toHaveTextContent("pi_dash_api_abc123");
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent(TOKEN_BACKUP_FAILED_NOTICE));
  });

  it("keeps the reveal open on Escape and closes it through the explicit Close action", async () => {
    const created = createdToken();
    const onClose = vi.fn();
    const { wrapper } = harness((url) => json(url, JSON.stringify(created), 201));
    Object.assign(URL, { createObjectURL: vi.fn(() => "blob:esc"), revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const user = typingUser();
    render(<CreateTokenDialog open onClose={onClose} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await screen.findByRole("heading", { name: "Key created" });
    await user.keyboard("{Escape}");
    expect(screen.getByRole("heading", { name: "Key created" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps the reveal open when the backdrop is pressed", async () => {
    const created = createdToken();
    const onClose = vi.fn();
    const { wrapper } = harness((url) => json(url, JSON.stringify(created), 201));
    Object.assign(URL, { createObjectURL: vi.fn(() => "blob:backdrop"), revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const user = typingUser();
    render(<CreateTokenDialog open onClose={onClose} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await screen.findByRole("heading", { name: "Key created" });
    const backdrop = document.querySelector("[data-base-ui-backdrop]") ?? document.body;
    fireEvent.pointerDown(backdrop as Element);
    fireEvent.click(backdrop as Element);
    expect(screen.getByRole("heading", { name: "Key created" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("drops the secret from the mutation cache once the reveal closes", async () => {
    const created = createdToken();
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
      return json(url, JSON.stringify(created), 201);
    }) as never);
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <React.Fragment>{children}</React.Fragment>
        <ToastHost manager={getToastManager()} />
      </QueryClientProvider>
    );
    Object.assign(URL, { createObjectURL: vi.fn(() => "blob:cache"), revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const user = typingUser();
    render(<CreateTokenDialog open onClose={() => undefined} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    await screen.findByRole("heading", { name: "Key created" });
    expect(screen.getByTestId("token-secret")).toHaveTextContent("pi_dash_api_abc123");
    await user.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Key created" })).toBeNull());
    const mutations = queryClient
      .getMutationCache()
      .getAll()
      .map((mutation) => mutation.state.data);
    expect(mutations.some((data) => JSON.stringify(data ?? null).includes("pi_dash_api_abc123"))).toBe(false);
  });

  it("rejects a submit while the request is in flight and ignores pre-reveal dismissal", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { wrapper } = harness(async (url) => {
      await gate;
      return json(url, JSON.stringify(createdToken()), 201);
    });
    Object.assign(URL, { createObjectURL: vi.fn(() => "blob:pending"), revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    const onClose = vi.fn();
    const user = typingUser();
    render(<CreateTokenDialog open onClose={onClose} now={NOW} />, { wrapper });
    await user.type(screen.getByPlaceholderText("Title"), "Deploy bot");
    await user.click(screen.getByRole("switch", { name: "Never expires" }));
    await user.click(screen.getByRole("button", { name: "Generate token" }));
    const pending = screen.getByRole("button", { name: "Generate token" });
    expect(pending).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
    await act(async () => release?.());
    await screen.findByRole("heading", { name: "Key created" });
  });
});
