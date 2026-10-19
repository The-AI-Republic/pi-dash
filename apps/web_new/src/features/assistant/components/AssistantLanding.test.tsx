// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assistant landing (AGT-038–AGT-040): greeting plus composer when keyed,
// the setup card when not, one conversation per submit with the first
// message handed on, and a failure notice that creates nothing.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AssistantThread, LlmConfig } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sessionKeys, setSessionClient } from "../../../core/session/queries.js";
import { assistantKeys } from "../api/keys.js";
import { AssistantLanding } from "./AssistantLanding.js";

const notified = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error };
});

const THREAD: AssistantThread = {
  id: "11111111-1111-4111-8111-111111111111",
  title: "",
  is_archived: false,
  has_active_turn: false,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
};

const KEYED: LlmConfig = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};

const KEYLESS: LlmConfig = { ...KEYED, model_name: "", has_api_key: false };

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const posts: string[] = [];
let createResponse: (() => Promise<unknown>) | null = null;

function renderLanding(options: { config?: LlmConfig; role?: number; seedConfig?: boolean } = {}) {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  posts.length = 0;
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/ai-assistant/threads/") && init.method === "POST") {
      posts.push(url);
      const body = await (createResponse ?? (async () => THREAD))();
      const failed = typeof body === "object" && body !== null && "error" in body;
      return {
        status: failed ? 500 : 201,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(body),
      };
    }
    return { status: 500, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "boom" }) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(sessionKeys.workspaces, [
    {
      id: "33333333-3333-4333-8333-333333333333",
      name: "Acme",
      slug: "acme",
      logo_url: null,
      total_members: 3,
      role: options.role ?? 15,
    },
  ]);
  if (options.seedConfig !== false) {
    queryClient.setQueryData(assistantKeys.config("acme"), options.config ?? KEYED);
    queryClient.setQueryData(assistantKeys.threads("acme"), []);
  }
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const layout = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <Outlet />,
  });
  const landing = createRoute({
    getParentRoute: () => layout,
    path: "/assistant",
    component: () => <AssistantLanding workspaceSlug="acme" />,
  });
  const conversation = createRoute({
    getParentRoute: () => layout,
    path: "/assistant/$threadId",
    component: () => <div data-testid="conversation" />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([layout.addChildren([landing, conversation])]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: ["/acme/assistant"] }),
  });
  routerRef = router;
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router };
}

let routerRef: { state: { location: { pathname: string; state: unknown } } } | null = null;

afterEach(() => {
  notified.error.mockReset();
  createResponse = null;
  resetAppClient();
  routerRef = null;
});

describe("AssistantLanding", () => {
  it("greets a keyed member with the headline, caption and composer", async () => {
    renderLanding({ config: KEYED });
    expect(await screen.findByRole("heading", { name: "How can I help you today?" })).toBeInTheDocument();
    expect(screen.getByText("Ask about your issues, create work, or start a coding run.")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message the assistant" })).toBeInTheDocument();
    expect(screen.queryByTestId("assistant-setup-card")).not.toBeInTheDocument();
  });

  it("swaps in the setup card without a key and hides the chat entry", async () => {
    renderLanding({ config: KEYLESS });
    expect(await screen.findByTestId("assistant-setup-card")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "How can I help you today?" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Message the assistant" })).not.toBeInTheDocument();
  });

  it("shows the loading state until the config resolves", async () => {
    // Render the view directly so the pending read is observable before
    // the router would block the paint.
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    createAppClient("https://api.test", (async (url: string) => ({
      status: 500,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify({ error: "boom" }),
    })) as never);
    render(
      <QueryClientProvider client={queryClient}>
        <AssistantLanding workspaceSlug="acme" />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("assistant-landing")).toBeInTheDocument();
    expect(screen.queryByTestId("assistant-setup-card")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "How can I help you today?" })).not.toBeInTheDocument();
    // The refused read resolves to a failure; the card never flips into setup.
    await waitFor(() => expect(screen.queryByTestId("assistant-setup-card")).not.toBeInTheDocument());
  });

  it("starts exactly one conversation, locks while starting and hands the message on", async () => {
    const pending = deferred<AssistantThread>();
    createResponse = () => pending.promise;
    renderLanding();
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: " plan my week " } });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(box).toBeDisabled());
    // A second submit cannot fire while the create is in flight.
    fireEvent.keyDown(box, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    expect(posts).toHaveLength(1);
    pending.resolve(THREAD);
    expect(await screen.findByTestId("conversation")).toBeInTheDocument();
    expect(routerRef?.state.location.pathname).toBe(`/acme/assistant/${THREAD.id}`);
    // The handoff rides the router's history state next to its own keys.
    expect((routerRef!.state.location.state as { pendingMessage?: string }).pendingMessage).toBe("plan my week");
  });

  it("ignores blank submits", async () => {
    renderLanding();
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "   " } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(posts).toEqual([]);
    expect(routerRef?.state.location.pathname).toBe("/acme/assistant");
  });

  it("notices a failed start and stays put", async () => {
    createResponse = async () => ({ error: "parity thread-create failure" });
    renderLanding();
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(notified.error).toHaveBeenCalledWith("Unable to start chat", "parity thread-create failure");
    });
    expect(routerRef?.state.location.pathname).toBe("/acme/assistant");
  });

  it("surfaces a guest's refused create as a notice (backend-driven exclusion)", async () => {
    createResponse = async () => ({
      error: "role_not_allowed",
      detail: "The assistant is available to workspace members.",
    });
    // A guest's create is refused by the server with the assistant's
    // `{error, detail}` shape; the client lifts the detail into ApiError.
    renderLanding({ role: 5 });
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "guest hello" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => {
      expect(notified.error).toHaveBeenCalledWith(
        "Unable to start chat",
        "The assistant is available to workspace members."
      );
    });
    expect(routerRef?.state.location.pathname).toBe("/acme/assistant");
  });
});
