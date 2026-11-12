// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhooks list (parity/specs/workspace-settings.md §18/§24,
// WSET-019/041): heading and Add, loader, empty state with its own Add,
// rows linking to detail, the row switch's active-only patch, and the
// read-failure notice.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import type { Transport } from "@pidash/api-client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  WEBHOOKS_ADD_LABEL,
  WEBHOOKS_EMPTY_TITLE,
  WEBHOOKS_HEADING,
  WEBHOOKS_LOAD_FAILED,
  WEBHOOKS_ROW_SWITCH_LABEL,
  WebhooksListPage,
  webhookDetailHref,
} from "./WebhooksListPage.js";

const SLUG = "acme";
const ID = "223e4567-e89b-12d3-a456-426614174001";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: ({
      to,
      params,
      children,
      ...rest
    }: {
      to: string;
      params: Record<string, string>;
      children: React.ReactNode;
    }) => React.createElement("a", { href: substitute(to, params), ...rest }, children),
  };
});

function substitute(to: string, params: Record<string, string>): string {
  let href = to;
  for (const [key, value] of Object.entries(params)) href = href.replace(`$${key}`, value);
  return href;
}

function row(overrides: Record<string, unknown> = {}) {
  return {
    id: ID,
    url: "https://example.com/post",
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    project: true,
    issue: true,
    cycle: true,
    module: true,
    issue_comment: true,
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: unknown;
}

function renderPage(handler: (call: Call) => { status: number; body?: unknown } | "hold") {
  const calls: Call[] = [];
  const transport: Transport = async (url, init) => {
    const call = {
      method: init?.method ?? "GET",
      url,
      body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
    };
    calls.push(call);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const answer = handler(call);
    if (answer === "hold") return new Promise(() => undefined);
    return {
      status: answer.status,
      url,
      headers: { get: () => null },
      text: async () => (answer.body === undefined ? "" : JSON.stringify(answer.body)),
    };
  };
  createAppClient("https://api.test", transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <WebhooksListPage workspaceSlug={SLUG} />
    </QueryClientProvider>
  );
  return { ...view, calls, queryClient };
}

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("WebhooksListPage", () => {
  it("shows the heading, the Add control and a loader while the read is held", () => {
    renderPage(() => "hold");
    expect(screen.getByRole("heading", { name: WEBHOOKS_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Loading webhooks" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: WEBHOOKS_ADD_LABEL })).not.toBeInTheDocument();
  });

  it("offers the same creation action from the empty state", async () => {
    const user = userEvent.setup();
    renderPage(() => ({ status: 200, body: [] }));
    expect(await screen.findByText(WEBHOOKS_EMPTY_TITLE)).toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: WEBHOOKS_ADD_LABEL });
    expect(buttons).toHaveLength(2);
    await user.click(buttons[1] as HTMLElement);
    expect(await screen.findByText("Create webhook")).toBeInTheDocument();
  });

  it("renders a row per webhook linking to its detail page", async () => {
    renderPage(() => ({
      status: 200,
      body: [row(), row({ id: "223e4567-e89b-12d3-a456-426614174002", url: "https://example.com/b" })],
    }));
    const list = await screen.findByRole("list", { name: "Webhooks" });
    const links = within(list).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(["https://example.com/post", "https://example.com/b"]);
    expect(links[0]?.getAttribute("href")).toBe(webhookDetailHref(SLUG, ID));
  });

  it("writes only the active flag from the row switch", async () => {
    const { calls } = renderPage((call) =>
      call.method === "PATCH" ? { status: 200, body: row({ is_active: false }) } : { status: 200, body: [row()] }
    );
    const toggle = await screen.findByRole("switch", { name: WEBHOOKS_ROW_SWITCH_LABEL });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(calls.some((call) => call.method === "PATCH" && call.url.endsWith(`/webhooks/${ID}/`))).toBe(true)
    );
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({ is_active: false });
    await waitFor(() => expect(screen.getByRole("switch", { name: WEBHOOKS_ROW_SWITCH_LABEL })).not.toBeChecked());
  });

  it("explains a failed read", async () => {
    renderPage(() => ({ status: 500 }));
    expect(await screen.findByText(WEBHOOKS_LOAD_FAILED)).toBeInTheDocument();
  });

  it("opens the creation dialog from the heading", async () => {
    const user = userEvent.setup();
    renderPage(() => ({ status: 200, body: [row()] }));
    await screen.findByRole("list", { name: "Webhooks" });
    await user.click(screen.getByRole("button", { name: WEBHOOKS_ADD_LABEL }));
    expect(await screen.findByText("Create webhook")).toBeInTheDocument();
  });
});
