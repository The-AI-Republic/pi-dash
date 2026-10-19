// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat page composition (NEWFRONT-549, RUN-025/026/028/032): the runner
// comes from props, history is listed and selectable, selection reaches
// the address, warm-up runs for the resolving conversation, and the
// header/composer follow the live state.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const hooks = vi.hoisted(() => ({
  useChatRunner: vi.fn(),
  useChatSessions: vi.fn(),
  useChatMessages: vi.fn(),
  useNewChat: vi.fn(),
  useWarmUp: vi.fn(),
  useDocumentHead: vi.fn(),
}));
const area = vi.hoisted(() => ({ value: { workspaceId: "ws-id", allowed: true, access: "allowed" } }));

vi.mock("../area.js", () => ({ useRunnersArea: () => area.value }));
vi.mock("../../../shared/shell/index.js", () => ({ useDocumentHead: hooks.useDocumentHead }));
vi.mock("./queries.js", () => ({
  useChatRunner: hooks.useChatRunner,
  useChatSessions: hooks.useChatSessions,
  useChatMessages: hooks.useChatMessages,
}));
vi.mock("./useWarmUp.js", () => ({ useWarmUp: hooks.useWarmUp }));
vi.mock("./useNewChat.js", () => ({ useNewChat: hooks.useNewChat }));

import { makeRunner, makeSession } from "./harness.js";
import { RUNNER_CHAT_PAGE_TITLE, RunnerChatPage, runnerChatDocumentTitle } from "./RunnerChatPage.js";

const runner = makeRunner();
const openSession = makeSession({ id: "open", last_message_at: "2026-10-18T11:00:00Z" });
const closedSession = makeSession({ id: "closed", status: "closed", last_message_at: "2026-10-18T12:00:00Z" });
const newChatStart = vi.fn(async () => undefined);

beforeEach(() => {
  area.value = { workspaceId: "ws-id", allowed: true, access: "allowed" };
  hooks.useChatRunner.mockReturnValue({ data: runner, isPending: false });
  hooks.useChatSessions.mockReturnValue({ data: [openSession, closedSession], isPending: false });
  hooks.useChatMessages.mockReturnValue({
    data: [
      {
        id: "m-1",
        session: "open",
        role: "user",
        content: "hello runner",
        content_parts: [],
        status: "completed",
        local_item_id: "",
        local_turn_id: "",
        seq: 1,
        created_at: "2026-10-18T11:00:00Z",
        completed_at: null,
      },
    ],
    isPending: false,
  });
  hooks.useWarmUp.mockReturnValue(false);
  hooks.useNewChat.mockReturnValue({ busy: false, start: newChatStart });
  newChatStart.mockClear();
});

describe("runnerChatDocumentTitle", () => {
  it("prefixes the runner name when known", () => {
    expect(runnerChatDocumentTitle("alpha")).toBe("alpha - Runner chat");
    expect(runnerChatDocumentTitle(undefined)).toBe(RUNNER_CHAT_PAGE_TITLE);
    expect(runnerChatDocumentTitle("")).toBe(RUNNER_CHAT_PAGE_TITLE);
  });
});

describe("RunnerChatPage", () => {
  it("reads the runner, history and transcript for the address and scope", () => {
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(hooks.useChatRunner).toHaveBeenCalledWith("acme", "r-1", true);
    expect(hooks.useChatSessions).toHaveBeenCalledWith("acme", "ws-id", "r-1", true);
    expect(hooks.useChatMessages).toHaveBeenCalledWith("acme", "open", true);
    expect(hooks.useDocumentHead).toHaveBeenCalledWith({ title: "alpha - Runner chat", robots: "noindex, nofollow" });
    expect(screen.getByTestId("runner-chat-name")).toHaveTextContent("alpha");
    expect(screen.getByText("hello runner")).toBeInTheDocument();
  });

  it("auto-selects the open session and reports it to warm-up", () => {
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
      expect.objectContaining({ runnerId: "r-1", session: openSession, pendingId: null, enabled: true })
    );
  });

  it("honors a deep-linked closed session and blocks the composer", () => {
    render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId="closed" onSelectSession={vi.fn()} />
    );
    expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ session: closedSession }));
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");
    expect(screen.getByPlaceholderText("Message this runner…")).toBeDisabled();
  });

  it("writes an entry selection into the address", async () => {
    const onSelectSession = vi.fn();
    render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={onSelectSession} />
    );
    const history = screen.getByTestId("runner-chat-history");
    const closedEntry = history.querySelector('nav button[type="button"]');
    // Entries are newest-first: the closed one is the first entry.
    expect(closedEntry).toHaveTextContent("closed");
    await userEvent.click(closedEntry as HTMLElement);
    expect(onSelectSession).toHaveBeenCalledWith("closed");
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");
  });

  it("starts a new conversation from the history column", async () => {
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(newChatStart).toHaveBeenCalledTimes(1);
  });

  it("shows the busy create state on the control", () => {
    hooks.useWarmUp.mockReturnValue(true);
    hooks.useNewChat.mockReturnValue({ busy: true, start: newChatStart });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(screen.getByRole("button", { name: "New chat" })).toBeDisabled();
  });

  it("blocks the composer while the runner is still loading", () => {
    hooks.useChatRunner.mockReturnValue({ data: undefined, isPending: true });
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: true });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(screen.getByTestId("runner-chat-name")).toHaveTextContent("Runner");
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Loading");
  });

  it("names runner state reasons and lets a busy runner chat", () => {
    hooks.useChatRunner.mockReturnValue({ data: makeRunner({ status: "busy" }), isPending: false });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(screen.queryByTestId("runner-chat-block-reason")).toBeNull();
    expect(screen.getByText("busy")).toBeInTheDocument();
  });

  it("gates the composer while a turn is in flight", () => {
    const busyTurn = makeSession({ id: "open", active_turn_id: "t-1" });
    hooks.useChatSessions.mockReturnValue({ data: [busyTurn], isPending: false });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Response in progress");
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("keeps reads off until the area admits the viewer", () => {
    area.value = { workspaceId: "ws-id", allowed: false, access: "denied" };
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(hooks.useChatRunner).toHaveBeenCalledWith("acme", "r-1", false);
    expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ enabled: false }));
  });

  it("resets the pick and created session when the contact changes", async () => {
    const view = render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId="closed" onSelectSession={vi.fn()} />
    );
    expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ session: closedSession }));
    view.rerender(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-2" requestedSessionId={null} onSelectSession={vi.fn()} />
    );
    expect(hooks.useChatRunner).toHaveBeenLastCalledWith("acme", "r-2", true);
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ runnerId: "r-2", session: openSession })
      )
    );
  });

  it("shows a session created on this page when nothing else is open", async () => {
    let onCreated: ((session: ReturnType<typeof makeSession>) => void) | undefined;
    hooks.useWarmUp.mockImplementation((input: { onCreated: typeof onCreated }) => {
      onCreated = input.onCreated;
      return false;
    });
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: false });
    const created = makeSession({ id: "created", last_message_at: null });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    React.act(() => onCreated?.(created));
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ session: created }))
    );
  });

  it("keeps a locally created session listed as closed once a later read says so", async () => {
    let onCreated: ((session: ReturnType<typeof makeSession>) => void) | undefined;
    hooks.useWarmUp.mockImplementation((input: { onCreated: typeof onCreated }) => {
      onCreated = input.onCreated;
      return false;
    });
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: false });
    const created = makeSession({ id: "created" });
    const view = render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
    );
    React.act(() => onCreated?.(created));
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ session: created }))
    );
    hooks.useChatSessions.mockReturnValue({
      data: [makeSession({ id: "created", status: "closed" })],
      isPending: false,
    });
    view.rerender(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
    );
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ session: expect.objectContaining({ id: "created", status: "closed" }) })
      )
    );
  });

  it("renders an empty history before the session list answers", () => {
    hooks.useChatSessions.mockReturnValue({ data: undefined, isPending: true });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(screen.getByTestId("runner-chat-history-empty")).toBeInTheDocument();
    expect(hooks.useChatMessages).toHaveBeenLastCalledWith("acme", null, true);
  });

  it("waits on the transcript of the session already on screen", () => {
    hooks.useChatMessages.mockReturnValue({ data: undefined, isPending: true });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    expect(screen.getByTestId("runner-chat-transcript-empty")).toHaveTextContent("Waiting for the first message…");
  });

  it("hands the in-memory pick back to the address once the link carries it", async () => {
    const view = render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
    );
    const history = screen.getByTestId("runner-chat-history");
    await userEvent.click(history.querySelector('nav button[type="button"]') as HTMLElement);
    expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ pendingId: "closed" }));
    view.rerender(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId="closed" onSelectSession={vi.fn()} />
    );
    await waitFor(() => expect(hooks.useWarmUp).toHaveBeenLastCalledWith(expect.objectContaining({ pendingId: null })));
  });

  it("keeps the in-memory pick while the address names another entry", async () => {
    const view = render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
    );
    const history = screen.getByTestId("runner-chat-history");
    await userEvent.click(history.querySelector('nav button[type="button"]') as HTMLElement);
    view.rerender(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId="open" onSelectSession={vi.fn()} />
    );
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ pendingId: "closed", requestedId: "open" })
      )
    );
  });
});

describe("RunnerChatPage creation ordering (RUN-026)", () => {
  function captureCreated(): { current: ((session: ReturnType<typeof makeSession>) => void) | undefined } {
    const ref: { current: ((session: ReturnType<typeof makeSession>) => void) | undefined } = { current: undefined };
    hooks.useWarmUp.mockImplementation((input: { onCreated: typeof ref.current }) => {
      ref.current = input.onCreated;
      return false;
    });
    return ref;
  }

  it("shows an automatically created session as a fallback, not a pick", async () => {
    const createdRef = captureCreated();
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: false });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    React.act(() => createdRef.current?.(makeSession({ id: "created", last_message_at: null })));
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ session: expect.objectContaining({ id: "created" }), pendingId: null })
      )
    );
  });

  it("keeps a click made while the create was running", async () => {
    const createdRef = captureCreated();
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    const history = screen.getByTestId("runner-chat-history");
    await userEvent.click(history.querySelector('nav button[type="button"]') as HTMLElement);
    React.act(() => createdRef.current?.(makeSession({ id: "created", last_message_at: null })));
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ session: closedSession, pendingId: "closed" })
      )
    );
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");
  });

  it("keeps a deep link that arrives after the create resolved", async () => {
    const createdRef = captureCreated();
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: false });
    const view = render(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
    );
    React.act(() => createdRef.current?.(makeSession({ id: "created", last_message_at: null })));
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ session: expect.objectContaining({ id: "created" }) })
      )
    );
    hooks.useChatSessions.mockReturnValue({ data: [openSession, closedSession], isPending: false });
    view.rerender(
      <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId="closed" onSelectSession={vi.fn()} />
    );
    await waitFor(() =>
      expect(hooks.useWarmUp).toHaveBeenLastCalledWith(
        expect.objectContaining({ session: closedSession, requestedId: "closed" })
      )
    );
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");
  });
});
