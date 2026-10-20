// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation gate (AGT-039/AGT-040 + the AGT-038 handoff): the setup
// card fills the empty state without a key, the composer locks with the
// reminder line, a keyed conversation shows the plain hint, and the
// one-shot first message is delivered exactly once per thread.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssistantMessage, AssistantSendResponse, LlmConfig } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { assistantKeys } from "../api/keys.js";
import { ConversationGate } from "./ConversationGate.js";

const notified = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error };
});

const THREAD_ID = "11111111-1111-4111-8111-111111111111";

const KEYED: LlmConfig = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};
const KEYLESS: LlmConfig = { ...KEYED, model_name: "", has_api_key: false };

const USER_MESSAGE: AssistantMessage = {
  id: "22222222-2222-4222-8222-222222222222",
  role: "user",
  content: "hello",
  status: "completed",
  seq: 1,
  turn_id: null,
  payload: {},
  created_at: "2026-10-01T00:00:00Z",
  completed_at: "2026-10-01T00:00:00Z",
};

const SEND_RESPONSE: AssistantSendResponse = {
  turn: { id: "33333333-3333-4333-8333-333333333333", status: "queued" },
  message: USER_MESSAGE,
};

const sends: string[] = [];
let sendStatus = 202;
let sendBody: unknown = SEND_RESPONSE;

function renderGate(
  options: {
    config?: LlmConfig;
    state?: unknown;
    threadId?: string;
    hasTranscript?: boolean;
    /** Leave the config read pending forever (the cold-open paint). */
    pendingConfig?: boolean;
    /** Skip the seeded config so the read has to resolve on its own. */
    seedConfig?: boolean;
  } = {}
) {
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/messages/") && init.method === "POST") {
      sends.push(url);
      return {
        status: sendStatus,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(sendBody),
      };
    }
    if (url.endsWith("/ai-assistant/config/") && options.pendingConfig === true) {
      return new Promise<never>(() => {});
    }
    return { status: 500, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "boom" }) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedConfig !== false) {
    queryClient.setQueryData(assistantKeys.config("acme"), options.config ?? KEYED);
  }
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => (
      <ConversationGate
        workspaceSlug="acme"
        threadId={options.threadId ?? THREAD_ID}
        state={options.state}
        {...(options.hasTranscript === true ? { hasTranscript: true, transcript: <p>transcript</p> } : {})}
      />
    ),
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: ["/acme"] }),
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  return { router };
}

afterEach(() => {
  resetHarness();
});

function resetHarness(): void {
  sends.length = 0;
  sendStatus = 202;
  sendBody = SEND_RESPONSE;
  notified.error.mockReset();
  resetAppClient();
}

describe("ConversationGate", () => {
  it("marks the thread unsettled until the config read resolves", async () => {
    renderGate({ seedConfig: false, pendingConfig: true });
    expect(await screen.findByTestId("assistant-thread")).toHaveAttribute("data-settled", "false");
    expect(screen.queryByTestId("assistant-setup-card")).not.toBeInTheDocument();
  });

  it("shows the setup card and the locked composer without a key", async () => {
    renderGate({ config: KEYLESS });
    expect(await screen.findByTestId("assistant-setup-card")).toBeInTheDocument();
    expect(screen.getByTestId("assistant-thread")).toHaveAttribute("data-settled", "true");
    expect(screen.queryByTestId("assistant-empty-hint")).not.toBeInTheDocument();
    expect(screen.getByTestId("assistant-composer-reason")).toHaveTextContent(
      "Please set your API key in Settings first to start using AI Assistant."
    );
    expect(screen.getByRole("textbox", { name: "Message the assistant" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("shows the plain empty hint and an unlocked composer with a key", async () => {
    renderGate({ config: KEYED });
    expect(await screen.findByTestId("assistant-empty-hint")).toHaveTextContent(
      "Ask about your issues, create work, or start a coding run."
    );
    expect(screen.queryByTestId("assistant-setup-card")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Message the assistant" })).toBeEnabled();
    expect(screen.queryByTestId("assistant-composer-reason")).not.toBeInTheDocument();
  });

  it("lets the thread-view slice render its transcript instead of the empty hint", async () => {
    renderGate({ config: KEYED, hasTranscript: true });
    expect(await screen.findByText("transcript")).toBeInTheDocument();
    expect(screen.queryByTestId("assistant-empty-hint")).not.toBeInTheDocument();
  });

  it("delivers the handed-off first message once and clears the state", async () => {
    const { router } = renderGate({ config: KEYED, state: { pendingMessage: "  plan my week " } });
    await waitFor(() => expect(sends).toHaveLength(1));
    expect(sends[0]).toContain(`/api/workspaces/acme/ai-assistant/threads/${THREAD_ID}/messages/`);
    await waitFor(() =>
      expect((router.state.location.state as { pendingMessage?: string }).pendingMessage).toBeUndefined()
    );
  });

  it("does not deliver when the state carries no message", async () => {
    renderGate({ config: KEYED, state: {} });
    await screen.findByTestId("assistant-empty-hint");
    expect(sends).toEqual([]);
  });

  it("holds a typed draft without sending it from the shell", async () => {
    renderGate({ config: KEYED });
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    await userEvent.type(box, "hello{Enter}");
    expect(box).toHaveValue("hello");
    expect(sends).toEqual([]);
  });

  it("notices a first message the server refused", async () => {
    sendStatus = 422;
    sendBody = { error: "turn_active", detail: "A turn is already running." };
    renderGate({ config: KEYED, state: { pendingMessage: "hello" } });
    await waitFor(() => {
      expect(notified.error).toHaveBeenCalledWith("Could not send your message", "A turn is already running.");
    });
  });
});
