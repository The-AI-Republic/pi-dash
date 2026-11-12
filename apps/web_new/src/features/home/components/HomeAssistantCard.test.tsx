// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dashboard assistant card (NEWFRONT-603, parity/specs/agents.md §9
// AGT-048): quick entry with suggestions and recent conversations, the
// keyless refusal notice, and the guest absence.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AssistantThread, LlmConfig } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/index.js";
import { assistantKeys } from "../../assistant/index.js";
import { sessionKeys, setSessionClient } from "../../../core/session/queries.js";
import { cardAskOutcome, HomeAssistantCard, recentConversations } from "./HomeAssistantCard.js";

const notified = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notified.error, notifySuccess: notified.success };
});

function makeThread(overrides: Partial<AssistantThread> & Pick<AssistantThread, "id">): AssistantThread {
  return {
    title: `Conversation ${overrides.id}`,
    is_archived: false,
    has_active_turn: false,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    ...overrides,
  };
}

const THREAD_1 = makeThread({ id: "11111111-1111-4111-8111-111111111111", title: "Sprint summary" });
const ARCHIVED_THREAD = makeThread({
  id: "22222222-2222-4222-8222-222222222222",
  title: "Old planning",
  is_archived: true,
});

const CONFIGURED: LlmConfig = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};

const UNCONFIGURED: LlmConfig = { ...CONFIGURED, model_name: "", has_api_key: false };

function workspaceRow(role: number): Record<string, unknown> {
  return {
    id: "33333333-3333-4333-8333-333333333333",
    name: "Acme",
    slug: "acme",
    logo_url: null,
    total_members: 3,
    role,
  };
}

interface CreateCall {
  url: string;
  method: string;
}

const calls: CreateCall[] = [];
let threadCreateStatus = 201;
let threadCreateBody: unknown = makeThread({ id: "44444444-4444-4444-8444-444444444444" });
let threadCreateGate: Promise<unknown> | null = null;

function renderCard(options: {
  role?: number;
  seedWorkspaces?: boolean;
  threads?: AssistantThread[];
  config?: LlmConfig;
}): void {
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  calls.length = 0;
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    calls.push({ url, method: init.method ?? "GET" });
    if (url.endsWith("/ai-assistant/threads/") && init.method === "POST") {
      const body = threadCreateGate === null ? threadCreateBody : await threadCreateGate;
      return {
        status: threadCreateStatus,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(body),
      };
    }
    return { status: 500, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "boom" }) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seedWorkspaces !== false) {
    queryClient.setQueryData(sessionKeys.workspaces, [workspaceRow(options.role ?? ROLE_MEMBER)]);
  }
  if (options.threads !== undefined) queryClient.setQueryData(assistantKeys.threads("acme"), options.threads);
  if (options.config !== undefined) queryClient.setQueryData(assistantKeys.config("acme"), options.config);
  // The card navigates into a started conversation, so it mounts under a
  // minimal router with that address registered.
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const homeRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => (
      <div data-testid="card-host">
        <HomeAssistantCard workspaceSlug="acme" />
      </div>
    ),
  });
  const assistantRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws/assistant/$threadId",
    component: () => <div data-testid="conversation" />,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([homeRoute, assistantRoute]),
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: ["/acme"] }),
  });
  routerRef = router;
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}

let routerRef: { state: { location: { pathname: string } } } | null = null;

afterEach(() => {
  routerRef = null;
});

afterEach(() => {
  notified.error.mockReset();
  notified.success.mockReset();
  threadCreateStatus = 201;
  threadCreateBody = makeThread({ id: "44444444-4444-4444-8444-444444444444" });
  threadCreateGate = null;
  resetAppClient();
});

describe("recentConversations", () => {
  it("drops archived rows and caps the list at five", () => {
    const rows = Array.from({ length: 7 }, (_, index) => makeThread({ id: `t-${index}` }));
    const archived = makeThread({ id: "gone", is_archived: true });
    expect(recentConversations([archived, ...rows]).map((row) => row.id)).toEqual(["t-0", "t-1", "t-2", "t-3", "t-4"]);
    expect(recentConversations(undefined)).toEqual([]);
  });
});

describe("cardAskOutcome", () => {
  it("decides blank, busy, keyless and start", () => {
    expect(cardAskOutcome("   ", true, false)).toBe("blank");
    expect(cardAskOutcome("hello", true, true)).toBe("busy");
    expect(cardAskOutcome("hello", false, false)).toBe("keyless");
    expect(cardAskOutcome("  hello ", true, false)).toBe("start");
  });
});

describe("HomeAssistantCard", () => {
  it("offers the entry, the pinned chips and up to five recents for members", async () => {
    const threads = Array.from({ length: 6 }, (_, index) =>
      makeThread({ id: `thread-${index}`, title: `Recent ${index}` })
    );
    renderCard({ threads: [ARCHIVED_THREAD, ...threads], config: CONFIGURED });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "ready");
    const input = screen.getByRole("textbox", { name: "Ask the assistant" });
    expect(input).toHaveValue("");
    expect(screen.getByRole("button", { name: "Ask" })).toBeDisabled();
    for (const chip of ["What's assigned to me?", "Create an issue in…", "Summarize open issues in…"]) {
      expect(screen.getByRole("button", { name: chip })).toBeInTheDocument();
    }
    const recents = screen.getAllByRole("link").filter((link) => link.getAttribute("href")?.includes("/assistant/"));
    expect(recents).toHaveLength(5);
    expect(recents.map((link) => link.textContent)).toEqual([
      "Recent 0",
      "Recent 1",
      "Recent 2",
      "Recent 3",
      "Recent 4",
    ]);
    expect(recents[0]).toHaveAttribute("href", "/acme/assistant/thread-0");
    expect(screen.queryByRole("link", { name: "Old planning" })).not.toBeInTheDocument();
  });

  it("fills the draft from a chip without sending and enables Ask", async () => {
    renderCard({ threads: [], config: CONFIGURED });
    await screen.findByRole("region", { name: "Assistant" });
    fireEvent.click(screen.getByRole("button", { name: "Create an issue in…" }));
    expect(screen.getByRole("textbox", { name: "Ask the assistant" })).toHaveValue("Create an issue in…");
    expect(screen.getByRole("button", { name: "Ask" })).toBeEnabled();
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
  });

  it("falls back to the untitled label and shows the setup reminder without a key", async () => {
    renderCard({ threads: [makeThread({ id: "blank", title: "   " })], config: UNCONFIGURED });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "setup");
    expect(screen.getByText(/connect a model/i)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Set up the assistant" })).toHaveAttribute("href", "/acme/assistant");
    expect(screen.getByRole("link", { name: "Untitled conversation" })).toBeInTheDocument();
    // Chips stay available: a keyless pick fills the draft and the ask refuses with the notice.
    expect(screen.getByRole("button", { name: "What's assigned to me?" })).toBeInTheDocument();
  });

  it("refuses a keyless ask with the reminder and starts nothing", async () => {
    renderCard({ threads: [], config: UNCONFIGURED });
    await screen.findByRole("region", { name: "Assistant" });
    fireEvent.change(screen.getByRole("textbox", { name: "Ask the assistant" }), { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    expect(notified.error).toHaveBeenCalledWith(
      "API key required",
      "Please set your API key in Settings first to start using AI Assistant."
    );
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
  });

  it("tells members connecting a model is needed without sending", async () => {
    renderCard({ threads: [], config: UNCONFIGURED });
    await screen.findByRole("region", { name: "Assistant" });
    fireEvent.change(screen.getByRole("textbox", { name: "Ask the assistant" }), { target: { value: "   " } });
    expect(screen.getByRole("button", { name: "Ask" })).toBeDisabled();
  });

  it("starts a conversation and hands the draft on for a configured member", async () => {
    renderCard({ threads: [], config: CONFIGURED });
    await screen.findByRole("region", { name: "Assistant" });
    fireEvent.change(screen.getByRole("textbox", { name: "Ask the assistant" }), {
      target: { value: " plan my week " },
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Ask the assistant" }), { key: "Enter" });
    await waitFor(() => {
      expect(calls.filter((call) => call.method === "POST")).toHaveLength(1);
    });
    // The card hands the draft to the new conversation and leaves the page.
    expect(await screen.findByTestId("conversation")).toBeInTheDocument();
    expect(routerRef?.state.location.pathname).toBe("/acme/assistant/44444444-4444-4444-8444-444444444444");
  });

  it("waits for both reads before rendering and absorbs an Enter while starting", async () => {
    // The threads read resolved but the config read has not: the card stays
    // absent instead of painting a half-known setup state.
    renderCard({ threads: [THREAD_1] });
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();

    let release!: (body: unknown) => void;
    threadCreateGate = new Promise<unknown>((resolve) => {
      release = resolve;
    });
    renderCard({ threads: [], config: CONFIGURED });
    const input = await screen.findByRole("textbox", { name: "Ask the assistant" });
    fireEvent.change(input, { target: { value: "first" } });
    expect(screen.getByRole("button", { name: "Ask" })).toBeEnabled();
    fireEvent.keyDown(input, { key: "Enter" });
    // The Ask button is disabled while starting, but the single-line input
    // still takes keys; a second Enter must be absorbed, not start again.
    fireEvent.change(input, { target: { value: "second" } });
    fireEvent.keyDown(input, { key: "Enter" });
    release(makeThread({ id: "44444444-4444-4444-8444-444444444444" }));
    await waitFor(() => expect(calls.filter((call) => call.method === "POST")).toHaveLength(1));
  });

  it("asks on Enter but never on a modifier chord", async () => {
    renderCard({ threads: [], config: CONFIGURED });
    const input = await screen.findByRole("textbox", { name: "Ask the assistant" });
    fireEvent.change(input, { target: { value: "chord" } });
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(input, { key: "Enter", metaKey: true });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
    expect(routerRef?.state.location.pathname).toBe("/acme");
  });

  it("shows the start failure detail in the notice", async () => {
    threadCreateStatus = 403;
    threadCreateBody = { error: "role_not_allowed", detail: "The assistant is available to workspace members." };
    renderCard({ threads: [], config: CONFIGURED });
    await screen.findByRole("region", { name: "Assistant" });
    fireEvent.change(screen.getByRole("textbox", { name: "Ask the assistant" }), { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    await waitFor(() => {
      expect(notified.error).toHaveBeenCalledWith(
        "Unable to start chat",
        "The assistant is available to workspace members."
      );
    });
  });

  it("treats a hosted or account lane as ready without a self-supplied key (CLOUD-037)", async () => {
    // The cloud card reads the same lane the assistant entries do: a lane
    // that carries its own credential is ready even with no key stored.
    const hosted: LlmConfig = {
      ...UNCONFIGURED,
      credential_mode: "openhub",
      has_openhub_access: true,
      has_byok_api_key: false,
    };
    renderCard({ threads: [], config: hosted });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "ready");
    expect(screen.queryByText(/connect a model/i)).not.toBeInTheDocument();

    renderCard({
      threads: [],
      config: { ...UNCONFIGURED, credential_mode: "codex", has_codex_token: true, has_byok_api_key: false },
    });
    expect(await screen.findByRole("region", { name: "Assistant" })).toHaveAttribute("data-assistant-state", "ready");
  });

  it("keeps the setup reminder when the active lane has no credential (CLOUD-037)", async () => {
    renderCard({
      threads: [],
      config: { ...UNCONFIGURED, credential_mode: "openhub", has_openhub_access: false },
    });
    const card = await screen.findByRole("region", { name: "Assistant" });
    expect(card).toHaveAttribute("data-assistant-state", "setup");
    fireEvent.change(screen.getByRole("textbox", { name: "Ask the assistant" }), { target: { value: "hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    expect(notified.error).toHaveBeenCalledWith("API key required", expect.stringContaining("Please set your API key"));
    expect(calls.filter((call) => call.method === "POST")).toEqual([]);
  });

  it("renders the same card for admins", async () => {
    renderCard({ role: ROLE_ADMIN, threads: [THREAD_1], config: CONFIGURED });
    expect(await screen.findByRole("region", { name: "Assistant" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ask the assistant" })).toBeInTheDocument();
  });

  it("hides the card from guests without fetching", async () => {
    governGuestRender();
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();
  });

  it("renders nothing while permissions load", () => {
    renderCard({ seedWorkspaces: false });
    expect(screen.queryByRole("region", { name: "Assistant" })).not.toBeInTheDocument();
  });
});

function governGuestRender(): void {
  const urls: string[] = [];
  setSessionClient({
    getJson: () => new Promise<never>(() => {}),
    parse: (_schema: unknown, data: unknown) => data,
  } as never);
  createAppClient("https://api.test", (async (url: string) => {
    urls.push(url);
    return { status: 403, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "no" }) };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(sessionKeys.workspaces, [workspaceRow(ROLE_GUEST)]);
  render(
    <QueryClientProvider client={queryClient}>
      <HomeAssistantCard workspaceSlug="acme" />
    </QueryClientProvider>
  );
  expect(urls).toEqual([]);
}
