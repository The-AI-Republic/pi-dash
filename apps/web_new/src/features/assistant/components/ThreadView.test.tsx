// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Conversation transcript (AGT-042/043/046/047/049/053/054): rows render
// by role, markdown replies, an ellipsis while streaming, tinted error
// rows, tool rows with deep links, inline skipped-server notices, the empty
// hint, sending/stop, inline failures and the one-shot handoff.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { AssistantMessage, LlmConfig } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { sttConfigKeys } from "../../../shared/dictation/index.js";
import { assistantKeys } from "../api/keys.js";
import { transcriptKey } from "../api/threadKeys.js";
import { ThreadView, followTail, followTailWhileGrowing, isFollowingTail } from "./ThreadView.js";

const stream = vi.hoisted(() => ({
  subscriptions: [] as { close: () => void; handlers: { onEvent: (event: unknown) => void } }[],
}));

vi.mock("../api/events.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/events.js")>();
  return {
    ...actual,
    subscribeAssistantEvents: (
      _slug: string,
      _thread: string,
      _after: number,
      handlers: { onEvent: (event: unknown) => void }
    ) => {
      const entry = { close: vi.fn(), handlers };
      stream.subscriptions.push(entry);
      return { close: entry.close };
    },
  };
});

const THREAD_ID = "11111111-1111-4111-8111-111111111111";
const KEYED: LlmConfig = {
  provider_kind: "openai",
  base_url: "https://api.test",
  model_name: "pilot",
  has_api_key: true,
  last_verified_at: null,
};

function row(overrides: Partial<AssistantMessage> = {}): AssistantMessage {
  return {
    id: "row-1",
    role: "assistant",
    content: "",
    status: "completed",
    seq: 1,
    turn_id: null,
    payload: {},
    created_at: "2026-10-01T00:00:00Z",
    completed_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

interface HarnessOptions {
  messages?: AssistantMessage[];
  state?: unknown;
  /** The thread list's `has_active_turn`; idle threads default to false. */
  active?: boolean;
  messageList?: () => Promise<unknown>;
  send?: (body: unknown) => Promise<unknown>;
  cancel?: () => Promise<unknown>;
  /** Seed for the speech read; unset reads fail and hide the mic. */
  sttSeed?: unknown;
  /** Transcription answer; unset uploads fail. */
  transcribe?: () => Promise<unknown>;
}

let serverHasActiveTurn = true;

function renderThread(options: HarnessOptions = {}) {
  const sent: unknown[] = [];
  const cancels: string[] = [];
  serverHasActiveTurn = options.active ?? false;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const path = new URL(url).pathname;
    if (path.endsWith("/ai-assistant/threads/") && init.method !== "POST") {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          JSON.stringify([
            {
              id: THREAD_ID,
              title: "Thread",
              is_archived: false,
              has_active_turn: serverHasActiveTurn,
              created_at: "2026-10-01T00:00:00Z",
              updated_at: "2026-10-01T00:00:00Z",
            },
          ]),
      };
    }
    if (path.endsWith("/messages/") && init.method === "POST") {
      sent.push(JSON.parse(init.body ?? "{}"));
      if (options.send !== undefined) return options.send(JSON.parse(init.body ?? "{}"));
      return {
        status: 202,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          JSON.stringify({
            turn: { id: "55555555-5555-4555-8555-555555555555", status: "queued" },
            message: row({
              id: "66666666-6666-4666-8666-666666666666",
              role: "user",
              content: "hello",
              seq: 2,
            }),
          }),
      };
    }
    if (url.endsWith("/cancel/") && init.method === "POST") {
      cancels.push(url);
      if (options.cancel !== undefined) return options.cancel();
      return { status: 204, url, headers: { get: () => null }, text: async () => "" };
    }
    if (path.endsWith("/messages/")) {
      const data = options.messageList === undefined ? (options.messages ?? []) : await options.messageList();
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(data) };
    }
    if (path.endsWith("/transcribe/") && init.method === "POST") {
      if (options.transcribe !== undefined) return options.transcribe();
      return { status: 500, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "boom" }) };
    }
    return { status: 500, url, headers: { get: () => null }, text: async () => JSON.stringify({ error: "boom" }) };
  }) as never);

  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(assistantKeys.config("acme"), KEYED);
  queryClient.setQueryData(transcriptKey("acme", THREAD_ID), options.messages ?? []);
  if (options.sttSeed !== undefined) queryClient.setQueryData(sttConfigKeys.all(), options.sttSeed);
  const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()();
  const wsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/$ws",
    component: () => <ThreadView workspaceSlug="acme" threadId={THREAD_ID} state={options.state} configured />,
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
  return { sent, cancels, router, queryClient };
}

function emit(event: Record<string, unknown>): void {
  stream.subscriptions.at(-1)?.handlers.onEvent(event);
}

beforeEach(() => {
  stream.subscriptions.length = 0;
});

afterEach(() => {
  resetAppClient();
  serverHasActiveTurn = true;
});

describe("ThreadView transcript", () => {
  it("shows the empty hint for a keyed thread with no rows", async () => {
    renderThread();
    expect(await screen.findByTestId("assistant-empty-hint")).toHaveTextContent(
      "Ask about your issues, create work, or start a coding run."
    );
  });

  it("renders user and assistant rows by role with markdown", async () => {
    renderThread({
      messages: [
        row({ id: "u", role: "user", content: "question", seq: 1 }),
        row({ id: "a", role: "assistant", content: "# Answer\n\n**bold**", seq: 2 }),
      ],
    });
    const list = await screen.findByTestId("assistant-transcript");
    const items = list.querySelectorAll(":scope > li");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveClass("justify-end");
    expect(items[1]).toHaveClass("justify-start");
    expect(screen.getByText("question")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Answer" })).toBeInTheDocument();
    expect(screen.getByText("bold").tagName).toBe("STRONG");
  });

  it("shows an ellipsis for a streaming reply without text", async () => {
    renderThread({ messages: [row({ content: "", status: "streaming" })] });
    expect(await screen.findByText("…")).toBeInTheDocument();
  });

  it("shows the quiet mark, never the status word, for a row without any text", async () => {
    // The renderer loads on demand, so a completed reply paints once before
    // its markdown exists; a row that stopped before its first chunk never
    // gets any. Both must stay on the typing mark, not print "completed".
    renderThread({ messages: [row({ id: "b", content: "", status: "completed" })] });
    const list = await screen.findByTestId("assistant-transcript");
    expect(list.textContent).toBe("…");
    expect(list).not.toHaveTextContent(/completed/);
  });

  it("tints error rows", async () => {
    renderThread({
      messages: [row({ id: "e", role: "error", content: "Could not reach the model.", status: "failed" })],
    });
    const bubble = await screen.findByText("Could not reach the model.");
    // Tinted with the app's own danger token, not the old app's class name.
    expect(bubble).toHaveClass("text-(--danger)");
  });

  it("falls back to a sentence for an empty error row", async () => {
    renderThread({ messages: [row({ id: "e", role: "error", content: "", status: "failed" })] });
    expect(await screen.findByText("The assistant hit an error.")).toBeInTheDocument();
  });

  it("renders tool rows and unknown roles as plain rows", async () => {
    renderThread({
      messages: [
        row({ id: "t", role: "tool_call", content: "Search issues", seq: 1 }),
        row({ id: "r", role: "tool_result", content: "3 results", seq: 2 }),
      ],
    });
    expect(await screen.findByText("Search issues")).toBeInTheDocument();
    expect(screen.getByText("3 results")).toBeInTheDocument();
  });

  it("renders tool-result deep links with their labels", async () => {
    renderThread({
      messages: [
        row({
          id: "r",
          role: "tool_result",
          content: "Found it",
          seq: 2,
          payload: {
            links: [
              { url_path: "/acme/assistant/t1", label: "Open thread" },
              { url_path: "/acme/projects/p/1/issues/i/2" },
            ],
          },
        }),
      ],
    });
    const labelled = await screen.findByRole("link", { name: "Open thread" });
    expect(labelled).toHaveAttribute("href", "/acme/assistant/t1");
    const fallback = screen.getByRole("link", { name: "View" });
    expect(fallback).toHaveAttribute("href", "/acme/projects/p/1/issues/i/2");
    // The activity text stays its own node, without the link labels.
    expect(screen.getByTestId("assistant-tool-text")).toHaveTextContent("Found it");
  });

  it("renders dropped servers as one inline line per server, never a toast", async () => {
    renderThread({ messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })] });
    await screen.findByTestId("assistant-transcript");
    act(() =>
      stream.subscriptions.at(-1)?.handlers.onEvent({
        id: 301,
        thread: THREAD_ID,
        message: null,
        seq: 301,
        kind: "tool_servers_skipped",
        payload: {
          servers: [
            { name: "closed", reason: "url_blocked" },
            { name: "dead", reason: "RuntimeError" },
          ],
        },
        created_at: "2026-01-01T00:00:00Z",
      })
    );
    const lines = await screen.findAllByTestId("assistant-notice-line");
    expect(lines.map((line) => line.textContent)).toEqual([
      "Tool server closed was unavailable for this reply (its URL is not allowed).",
      "Tool server dead was unavailable for this reply (RuntimeError).",
    ]);
    expect(screen.queryByTestId("toast-notification")).not.toBeInTheDocument();
  });

  it("keeps the scrollport on the newest row without a scrollIntoView host", () => {
    const original = Element.prototype.scrollIntoView;
    // jsdom leaves scrollIntoView undefined; a host that lacks it must still follow.
    Element.prototype.scrollIntoView = undefined as unknown as typeof Element.prototype.scrollIntoView;
    try {
      const body = document.createElement("div");
      Object.defineProperty(body, "scrollHeight", { value: 480, configurable: true });
      followTail(body, document.createElement("div"));
      expect(body.scrollTop).toBe(480);
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });

  it("does nothing when either the body or the tail marker is absent", () => {
    expect(() => followTail(null, document.createElement("div"))).not.toThrow();
    expect(() => followTail(document.createElement("div"), null)).not.toThrow();
  });

  it("scrolls the tail marker into view as rows arrive", async () => {
    const scrollIntoView = vi.fn();
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      renderThread({ messages: [row({ content: "hi" })] });
      await screen.findByTestId("assistant-transcript");
      expect(scrollIntoView).toHaveBeenCalled();
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });

  it("treats a scrollport within one line of the end as following the tail", () => {
    const body = document.createElement("div");
    Object.defineProperty(body, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(body, "clientHeight", { value: 400, configurable: true });
    body.scrollTop = 600;
    expect(isFollowingTail(body)).toBe(true);
    body.scrollTop = 590;
    expect(isFollowingTail(body)).toBe(true);
    body.scrollTop = 200;
    expect(isFollowingTail(body)).toBe(false);
  });

  it("re-pins the tail when the list grows after the first scroll", async () => {
    const body = document.createElement("div");
    Object.defineProperty(body, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(body, "clientHeight", { value: 400, configurable: true });
    body.scrollTop = 200;
    const tail = document.createElement("div");
    const dispose = followTailWhileGrowing(body, tail);
    try {
      // The reply's markdown lands: the list grows, so the port stops at the end.
      Object.defineProperty(body, "scrollHeight", { value: 1200, configurable: true });
      body.appendChild(document.createElement("span"));
      await waitFor(() => expect(body.scrollTop).toBe(1200));
    } finally {
      dispose();
    }
  });

  it("stops following once the reader scrolls up, and resumes at the end", async () => {
    const body = document.createElement("div");
    Object.defineProperty(body, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(body, "clientHeight", { value: 400, configurable: true });
    const tail = document.createElement("div");
    const dispose = followTailWhileGrowing(body, tail);
    try {
      body.appendChild(document.createElement("span"));
      await waitFor(() => expect(body.scrollTop).toBe(1000));
      body.scrollTop = 120;
      fireEvent.scroll(body);
      Object.defineProperty(body, "scrollHeight", { value: 1200, configurable: true });
      body.appendChild(document.createElement("span"));
      await act(async () => {
        await Promise.resolve();
      });
      expect(body.scrollTop).toBe(120);
      body.scrollTop = 800;
      fireEvent.scroll(body);
      Object.defineProperty(body, "scrollHeight", { value: 1400, configurable: true });
      body.appendChild(document.createElement("span"));
      await waitFor(() => expect(body.scrollTop).toBe(1400));
    } finally {
      dispose();
    }
  });

  it("does nothing when the host has no MutationObserver", () => {
    const original = globalThis.MutationObserver;
    // A host without the API still renders: the entries effect alone follows.
    vi.stubGlobal("MutationObserver", undefined);
    try {
      expect(() => followTailWhileGrowing(document.createElement("div"), document.createElement("div"))).not.toThrow();
    } finally {
      vi.stubGlobal("MutationObserver", original);
    }
  });
});

describe("ThreadView composer", () => {
  it("sends the trimmed draft on Enter and clears the box", async () => {
    const { sent } = renderThread();
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "  hello  " } });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(sent).toEqual([{ content: "hello" }]));
    expect(box).toHaveValue("");
  });

  it("keeps the newline on Shift+Enter and never sends a blank", async () => {
    const { sent } = renderThread();
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "line" } });
    fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
    expect(sent).toEqual([]);
    for (const modifier of ["ctrlKey", "metaKey", "altKey"] as const) {
      fireEvent.keyDown(box, { key: "Enter", [modifier]: true });
    }
    expect(sent).toEqual([]);
    fireEvent.change(box, { target: { value: "   " } });
    fireEvent.keyDown(box, { key: "Enter" });
    fireEvent.submit(box.closest("form") as HTMLFormElement);
    expect(sent).toEqual([]);
  });

  it("swaps send for stop while a turn runs and posts cancel once", async () => {
    const { cancels } = renderThread({
      messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })],
      active: true,
    });
    const stop = await screen.findByRole("button", { name: "Stop response" });
    expect(screen.queryByRole("button", { name: "Send message" })).not.toBeInTheDocument();
    fireEvent.click(stop);
    await waitFor(() => expect(cancels).toHaveLength(1));
    // Still locked until the server confirms the end of the turn.
    expect(screen.getByRole("button", { name: "Stop response" })).toBeInTheDocument();
  });

  it("still posts a plain Enter while a turn runs and shows the server's refusal", async () => {
    // The box stays editable during a turn and the old surface lets Enter
    // post; the server owns the race and answers `turn_active`, which lands
    // inline instead of the key being swallowed (AGT-049).
    const { sent } = renderThread({
      messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })],
      active: true,
      send: async () => ({
        status: 409,
        url: "https://api.test",
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ error: "turn_active" }),
      }),
    });
    await screen.findByRole("button", { name: "Stop response" });
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "second" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(sent).toEqual([{ content: "second" }]));
    expect(await screen.findByTestId("assistant-error-line")).toHaveTextContent("turn_active");
  });

  it("unlocks after stop when the server reports the turn ended without a new row", async () => {
    const list = [row({ id: "u", role: "user", content: "ask", seq: 1 })];
    const harness = renderThread({ messages: list, active: true });
    await screen.findByRole("button", { name: "Stop response" });
    fireEvent.click(screen.getByRole("button", { name: "Stop response" }));
    await waitFor(() => expect(harness.cancels).toHaveLength(1));
    // The cancel finalizes the turn with no assistant or error row; the
    // thread list's `has_active_turn: false` is the only confirmation.
    serverHasActiveTurn = false;
    expect(await screen.findByRole("button", { name: "Send message" }, { timeout: 4000 })).toBeInTheDocument();
  });

  it("unlocks when a cancelled event arrives", async () => {
    renderThread({ messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })], active: true });

    await screen.findByRole("button", { name: "Stop response" });
    emit({
      id: 9,
      thread: THREAD_ID,
      message: null,
      seq: 9,
      kind: "turn_cancelled",
      payload: { turn_id: "55555555-5555-4555-8555-555555555555" },
      created_at: "2026-01-01T00:00:00Z",
    });
    expect(await screen.findByRole("button", { name: "Send message" })).toBeInTheDocument();
  });

  it("appends streamed deltas in arrival order and dedupes replays", async () => {
    renderThread({ messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })], active: true });
    await screen.findByRole("button", { name: "Stop response" });
    emit({
      id: 10,
      thread: THREAD_ID,
      message: "reply",
      seq: 10,
      kind: "message_created",
      payload: { turn_id: "turn", message: row({ id: "reply", content: "", status: "streaming", seq: 2 }) },
      created_at: "2026-01-01T00:00:00Z",
    });
    const delta = (seq: number, text: string) => ({
      id: seq,
      thread: THREAD_ID,
      message: "reply",
      seq,
      kind: "assistant_delta",
      payload: { turn_id: "turn", params: { delta: text } },
      created_at: "2026-01-01T00:00:00Z",
    });
    emit(delta(11, "Hello "));
    emit(delta(11, "Hello "));
    emit(delta(12, "world"));
    expect(await screen.findByText("Hello world")).toBeInTheDocument();
  });

  it("shows a turn failure inline and unlocks", async () => {
    renderThread({ messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })], active: true });
    await screen.findByRole("button", { name: "Stop response" });
    emit({
      id: 20,
      thread: THREAD_ID,
      message: null,
      seq: 20,
      kind: "turn_failed",
      payload: { turn_id: "turn", error_code: "provider_unreachable", detail: "Could not reach the provider." },
      created_at: "2026-01-01T00:00:00Z",
    });
    expect(await screen.findByTestId("assistant-error-line")).toHaveTextContent("Could not reach the provider.");
    expect(await screen.findByRole("button", { name: "Send message" })).toBeInTheDocument();
  });

  it("falls back to the code and then a sentence for a failure", async () => {
    renderThread({ messages: [row({ id: "u", role: "user", content: "ask", seq: 1 })], active: true });
    await screen.findByRole("button", { name: "Stop response" });
    emit({
      id: 21,
      thread: THREAD_ID,
      message: null,
      seq: 21,
      kind: "turn_failed",
      payload: { turn_id: "turn", error_code: "provider_error", detail: "" },
      created_at: "2026-01-01T00:00:00Z",
    });
    expect(await screen.findByTestId("assistant-error-line")).toHaveTextContent("provider_error");
    emit({
      id: 22,
      thread: THREAD_ID,
      message: null,
      seq: 22,
      kind: "turn_failed",
      payload: { turn_id: "turn", error_code: "", detail: "" },
      created_at: "2026-01-01T00:00:00Z",
    });
    expect(await screen.findByTestId("assistant-error-line")).toHaveTextContent(
      "The assistant could not finish this reply."
    );
  });

  it("shows a send refusal inline without losing the transcript", async () => {
    renderThread({
      messages: [
        row({ id: "u", role: "user", content: "earlier", seq: 1 }),
        row({ id: "a", role: "assistant", content: "answered", seq: 2 }),
      ],
      send: async () => ({
        status: 409,
        url: "https://api.test",
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ error: "turn_active" }),
      }),
    });
    const box = await screen.findByRole("textbox", { name: "Message the assistant" });
    fireEvent.change(box, { target: { value: "again" } });
    fireEvent.keyDown(box, { key: "Enter" });
    expect(await screen.findByTestId("assistant-error-line")).toHaveTextContent("turn_active");
    expect(screen.getByText("earlier")).toBeInTheDocument();
  });
});

describe("ThreadView dictation", () => {
  const STT_KEYED = {
    base_url: "https://stt.test",
    model_name: "whisper",
    has_api_key: true,
    last_verified_at: null,
    enabled: true,
  };

  class FakeRecorder {
    state: RecordingState = "inactive";
    mimeType = "audio/webm";
    ondataavailable: ((event: { data: Blob }) => void) | null = null;
    onstop: (() => void) | null = null;

    start(): void {
      this.state = "recording";
    }

    stop(): void {
      this.state = "inactive";
      this.ondataavailable?.({ data: new Blob(["audio-bytes"], { type: this.mimeType }) });
      this.onstop?.();
    }
  }

  function stubDevices(): void {
    const track = { stop: vi.fn() };
    vi.stubGlobal("navigator", {
      mediaDevices: { getUserMedia: async () => ({ getTracks: () => [track] }) },
    });
    vi.stubGlobal("MediaRecorder", FakeRecorder);
  }

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("hides the microphone while the speech read fails", async () => {
    stubDevices();
    renderThread();
    await screen.findByTestId("assistant-empty-hint");
    expect(screen.queryByTestId("assistant-mic")).not.toBeInTheDocument();
    expect(screen.queryByTestId("assistant-dictation-hint")).not.toBeInTheDocument();
  });

  it("offers the microphone once speech is configured", async () => {
    stubDevices();
    renderThread({ sttSeed: STT_KEYED });
    expect(await screen.findByTestId("assistant-mic")).toHaveAttribute("aria-label", "Hold to dictate");
  });

  it("routes a keyless microphone to dictation settings", async () => {
    stubDevices();
    renderThread({ sttSeed: { ...STT_KEYED, has_api_key: false } });
    const mic = await screen.findByTestId("assistant-mic");
    expect(mic).toHaveAttribute("aria-label", "Set up voice dictation");
    expect(mic).toHaveAttribute("href", "/settings/profile/ai-assistant#voice-dictation");
    expect(screen.getByTestId("assistant-dictation-hint")).toHaveTextContent("Voice dictation isn't set up.");
  });

  it("links nowhere from a fully keyed conversation", async () => {
    stubDevices();
    renderThread({ sttSeed: STT_KEYED });
    await screen.findByTestId("assistant-mic");
    // No setup card, no setup hint, no keyed-mic link: the registry and
    // the keys live in settings, never in chat (AGT-060).
    expect(document.querySelectorAll('a[href*="/settings/"]')).toHaveLength(0);
  });

  it("transcribes a hold into the draft", async () => {
    stubDevices();
    renderThread({
      sttSeed: STT_KEYED,
      transcribe: async () => ({
        status: 200,
        url: "https://api.test/api/users/me/ai-assistant/transcribe/",
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ text: "spoken words" }),
      }),
    });
    const mic = await screen.findByTestId("assistant-mic");
    fireEvent.pointerDown(mic);
    await waitFor(() =>
      expect(screen.getByTestId("assistant-mic")).toHaveAttribute("aria-label", "Release to transcribe")
    );
    await act(async () => {
      await new Promise((done) => setTimeout(done, 600));
    });
    fireEvent.pointerUp(mic);
    const box = screen.getByRole("textbox", { name: "Message the assistant" });
    await waitFor(() => expect(box).toHaveValue("spoken words"));
  }, 15000);
});

describe("ThreadView handoff", () => {
  it("delivers the landing message once and clears the state", async () => {
    const { sent, router } = renderThread({ state: { pendingMessage: "  plan my week " } });
    await waitFor(() => expect(sent).toEqual([{ content: "plan my week" }]));
    await waitFor(() =>
      expect((router.state.location.state as { pendingMessage?: string }).pendingMessage).toBeUndefined()
    );
  });

  it("does nothing without a handoff", async () => {
    const { sent } = renderThread({ state: {} });
    await screen.findByTestId("assistant-empty-hint");
    expect(sent).toEqual([]);
  });
});
