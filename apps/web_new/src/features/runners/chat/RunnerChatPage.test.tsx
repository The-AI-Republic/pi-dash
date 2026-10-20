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
  useChatStream: vi.fn(),
  useChatActions: vi.fn(),
  useDictationConfig: vi.fn(),
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
vi.mock("./useChatStream.js", () => ({ useChatStream: hooks.useChatStream }));
vi.mock("./useChatActions.js", () => ({ useChatActions: hooks.useChatActions }));
vi.mock("../../../shared/dictation/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/dictation/index.js")>();
  return { ...original, useDictationConfig: hooks.useDictationConfig };
});

import { makeRunner, makeSession, testClient, wrapper } from "./harness.js";
import { RUNNER_CHAT_PAGE_TITLE, RunnerChatPage, runnerChatDocumentTitle } from "./RunnerChatPage.js";
import { APPROVAL_MODE_COPY, approvalModeKey } from "./approvalCopy.js";
import { getChatTransport, setChatTransport, type ChatTransport } from "./transport.js";

const runner = makeRunner();
const openSession = makeSession({ id: "open", last_message_at: "2026-10-18T11:00:00Z" });
const closedSession = makeSession({ id: "closed", status: "closed", last_message_at: "2026-10-18T12:00:00Z" });
const newChatStart = vi.fn(async () => undefined);
const send = vi.fn(async () => undefined);
const stop = vi.fn(async () => undefined);
const close = vi.fn(async () => undefined);
const dismissSendError = vi.fn();
const dismissStreamError = vi.fn();
const refetchSessions = vi.fn(async () => undefined);
const refetchMessages = vi.fn(async () => undefined);
const stream = {
  messages: [] as unknown[],
  activity: [] as { seq: number; label: string }[],
  approval: null as Record<string, unknown> | null,
  error: null as string | null,
  dismissError: dismissStreamError,
};
const actions = {
  sending: false,
  sendError: null as string | null,
  dismissSendError,
  send,
  stop,
  close,
};

function renderPage(props: Partial<React.ComponentProps<typeof RunnerChatPage>> = {}) {
  return render(
    <RunnerChatPage
      workspaceSlug="acme"
      runnerId="r-1"
      requestedSessionId={null}
      onSelectSession={vi.fn()}
      {...props}
    />,
    { wrapper: wrapper(testClient()) }
  );
}

function rerenderPage(
  view: ReturnType<typeof render>,
  props: Partial<React.ComponentProps<typeof RunnerChatPage>> = {}
): void {
  view.rerender(
    <RunnerChatPage
      workspaceSlug="acme"
      runnerId="r-1"
      requestedSessionId={null}
      onSelectSession={vi.fn()}
      {...props}
    />
  );
}

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
    refetch: refetchMessages,
  });
  hooks.useChatSessions.mockReturnValue({
    data: [openSession, closedSession],
    isPending: false,
    refetch: refetchSessions,
  });
  hooks.useWarmUp.mockReturnValue(false);
  hooks.useNewChat.mockReturnValue({ busy: false, start: newChatStart });
  newChatStart.mockClear();
  send.mockClear();
  stop.mockClear();
  close.mockClear();
  dismissSendError.mockClear();
  dismissStreamError.mockClear();
  refetchSessions.mockClear();
  refetchMessages.mockClear();
  stream.messages = hooks.useChatMessages().data;
  stream.activity = [];
  stream.error = null;
  stream.approval = null;
  actions.sending = false;
  actions.sendError = null;
  hooks.useChatStream.mockReturnValue(stream);
  hooks.useChatActions.mockReturnValue(actions);
  hooks.useDictationConfig.mockReturnValue({ data: undefined, isPending: false });
  globalThis.localStorage.clear();
  setChatTransport(null);
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
    expect(screen.getByRole("button", { name: "Stop response" })).toBeInTheDocument();
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
    hooks.useChatMessages.mockReturnValue({ data: undefined, isPending: true, refetch: refetchMessages });
    stream.messages = [];
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

describe("RunnerChatPage messaging (RUN-027/029/030/031)", () => {
  const box = () => screen.getByPlaceholderText("Message this runner…");

  it("clears the draft as it sends and puts it back when the send is refused", async () => {
    renderPage();
    await userEvent.type(box(), "ship it");
    expect(box()).toHaveValue("ship it");
    await userEvent.keyboard("{Enter}");
    expect(send).toHaveBeenCalledWith("ship it", expect.any(Function));
    expect(box()).toHaveValue("");

    const [firstCall] = send.mock.calls as unknown[][];
    const restore = firstCall?.[1] as ((draft: string) => void) | undefined;
    React.act(() => restore?.("ship it"));
    expect(box()).toHaveValue("ship it");
  });

  it("subscribes to the resolved conversation and renders its live rows", () => {
    hooks.useChatStream.mockReturnValue(stream);
    renderPage();
    expect(hooks.useChatStream).toHaveBeenLastCalledWith("open", expect.any(Array), expect.any(Object), "r-1");
    expect(hooks.useChatActions).toHaveBeenLastCalledWith(
      expect.objectContaining({ runnerId: "r-1", session: openSession })
    );
  });

  it("shows streamed markdown and the activity strip in the transcript", () => {
    stream.messages = [
      {
        id: "live-1",
        session: "open",
        role: "assistant",
        content: "**streamed** answer",
        content_parts: [],
        status: "streaming",
        local_item_id: "",
        local_turn_id: "",
        seq: 9,
        created_at: "2026-10-18T11:00:01Z",
        completed_at: null,
      },
    ];
    stream.activity = [{ seq: 8, label: "Ran: ls" }];
    renderPage();
    expect(document.querySelector('[data-role="assistant"] strong')).toHaveTextContent("streamed");
    expect(screen.getByTestId("runner-chat-activity")).toHaveTextContent("Ran: ls");
  });

  it("stops the running turn from the composer", async () => {
    hooks.useChatSessions.mockReturnValue({
      data: [makeSession({ id: "open", active_turn_id: "t-1" })],
      isPending: false,
      refetch: refetchSessions,
    });
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: "Stop response" }));
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("closes the conversation from the header and hides close while none is resolved", async () => {
    const view = renderPage();
    await userEvent.click(screen.getByRole("button", { name: "Close chat" }));
    expect(close).toHaveBeenCalledTimes(1);

    hooks.useChatRunner.mockReturnValue({ data: undefined, isPending: true });
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: true, refetch: refetchSessions });
    rerenderPage(view);
    expect(screen.queryByRole("button", { name: "Close chat" })).toBeNull();
  });

  it("names a stream failure and dismisses the banner through both channels", async () => {
    stream.error = "Chat frame did not match its contract";
    renderPage();
    const banner = screen.getByRole("alert");
    expect(banner).toHaveTextContent("Chat frame did not match its contract");
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(dismissStreamError).toHaveBeenCalledTimes(1);
    expect(dismissSendError).toHaveBeenCalledTimes(1);
  });

  it("falls back to the send error when the stream is quiet", () => {
    actions.sendError = "Message not sent";
    renderPage();
    expect(screen.getByRole("alert")).toHaveTextContent("Message not sent");
    expect(dismissStreamError).not.toHaveBeenCalled();
  });

  it("shows nothing above the composer while both channels are quiet", () => {
    renderPage();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("offers dictation only when the speech read is on and has a key", () => {
    hooks.useDictationConfig.mockReturnValue({ data: { enabled: true, has_api_key: true }, isPending: false });
    const { unmount } = renderPage();
    expect(hooks.useDictationConfig).toHaveBeenLastCalledWith(true);
    unmount();
    hooks.useDictationConfig.mockReturnValue({ data: { enabled: true, has_api_key: false }, isPending: false });
    renderPage();
    expect(hooks.useDictationConfig).toHaveBeenLastCalledWith(true);
  });

  it("ignores the parked approval on a transport that cannot decide one", () => {
    stream.approval = {
      id: 1,
      session: "open",
      message: null,
      seq: 1,
      kind: "chat_approval_request",
      payload: { local_approval_id: "a-1", approval_kind: "command_execution", command: "ls" },
      created_at: "2026-10-18T11:00:00Z",
    };
    renderPage();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByRole("group", { name: "Approval mode" })).toBeNull();
  });

  it("refreshes history and transcript from the stream's own refresh signal", () => {
    renderPage();
    const callbacks = (hooks.useChatStream.mock.calls.at(-1) as unknown[] | undefined)?.[2] as
      | { onHistoryRefresh: () => void; onApprovalCleared: () => void }
      | undefined;
    React.act(() => callbacks?.onHistoryRefresh());
    expect(refetchSessions).toHaveBeenCalledTimes(1);
    expect(refetchMessages).toHaveBeenCalledTimes(1);
    expect(callbacks?.onApprovalCleared()).toBeUndefined();
  });
});

describe("RunnerChatPage desktop approvals (RUN-034/035)", () => {
  const decide = vi.fn(async () => undefined as void);
  const localTransport = (owns: (runnerId: string) => boolean = () => true): ChatTransport => ({
    subscribeChatEvents: vi.fn(() => () => undefined),
    sendChatMessage: vi.fn(),
    cancelChat: vi.fn(async () => ({ ok: true })),
    closeChat: vi.fn(async () => openSession),
    setApprovalMode: vi.fn(),
    setRunnerApprovalMode: vi.fn(),
    decideChatApproval: decide,
    ownsRunner: owns,
  });
  const approval = {
    id: 9,
    session: "open",
    message: null,
    seq: 9,
    kind: "chat_approval_request",
    payload: {
      local_approval_id: "a-1",
      approval_kind: "command_execution",
      reason: "checks the build",
      expires_at: new Date(Date.now() + 60_000).toISOString(),
      command: "npm test",
    },
    created_at: new Date().toISOString(),
  };

  beforeEach(() => {
    decide.mockClear();
    setChatTransport(localTransport());
  });

  it("shows the mode control and the parked request", () => {
    stream.approval = approval;
    renderPage();
    expect(screen.getByRole("group", { name: "Approval mode" })).toBeInTheDocument();
    const prompt = screen.getByRole("alertdialog", { name: "Approval requested" });
    expect(prompt).toHaveTextContent("npm test");
    expect(prompt).toHaveTextContent("checks the build");
  });

  it("clears the prompt optimistically and answers through the transport", async () => {
    let settle: () => void = () => undefined;
    decide.mockImplementationOnce(() => new Promise<void>((resolve) => (settle = () => resolve())));
    stream.approval = approval;
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(decide).toHaveBeenCalledWith("open", "a-1", "accept");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await React.act(async () => settle());
  });

  it("puts the prompt back with the reason when the decision fails", async () => {
    decide.mockRejectedValueOnce(new Error("socket closed"));
    stream.approval = approval;
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(await screen.findByTestId("chat-approval-error")).toHaveTextContent("socket closed");
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
  });

  it("names a decision failure that carries nothing readable", async () => {
    decide.mockRejectedValueOnce({});
    stream.approval = approval;
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(await screen.findByTestId("chat-approval-error")).toHaveTextContent("did not reach the agent");
  });

  it("drops the prompt at the request's own deadline", async () => {
    vi.useFakeTimers();
    try {
      stream.approval = {
        ...approval,
        payload: { ...approval.payload, expires_at: new Date(Date.now() + 5_000).toISOString() },
      };
      renderPage();
      expect(screen.getByRole("alertdialog")).toBeInTheDocument();
      await React.act(async () => {
        vi.advanceTimersByTime(5_001);
      });
      expect(screen.queryByRole("alertdialog")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("drops the prompt when stop interrupts the turn", async () => {
    stream.approval = approval;
    // A turn in flight is what turns the send control into stop.
    hooks.useChatSessions.mockReturnValue({
      data: [{ ...openSession, active_message_id: "m-2" }, closedSession],
      isPending: false,
      refetch: refetchSessions,
    });
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: "Stop response" }));
    expect(stop).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("persists the chosen mode and hands it to the session and the runner", async () => {
    const transport = getChatTransport();
    renderPage();
    await userEvent.click(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label }));
    expect(globalThis.localStorage.getItem(approvalModeKey("r-1"))).toBe("ask");
    expect(transport.setApprovalMode).toHaveBeenCalledWith("open", "ask");
    expect(transport.setRunnerApprovalMode).toHaveBeenCalledWith("r-1", "ask");
  });

  it("loads the stored mode for the contact on screen", () => {
    globalThis.localStorage.setItem(approvalModeKey("r-1"), "workspace");
    renderPage();
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.workspace.label })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.full_access.label })).toHaveAttribute(
      "aria-pressed",
      "false"
    );
  });

  it("hands the stored mode to the session the page warms", () => {
    globalThis.localStorage.setItem(approvalModeKey("r-1"), "ask");
    const transport = getChatTransport();
    renderPage();
    expect(transport.setApprovalMode).toHaveBeenCalledWith("open", "ask");
    // Handed over at mount, before warm-up can create a conversation.
    expect(transport.setRunnerApprovalMode).toHaveBeenCalledWith("r-1", "ask");
  });

  it("keeps a server runner on the cloud path and hides the approval controls", () => {
    setChatTransport(localTransport((runnerId) => runnerId === "pidash-builtin"));
    renderPage();
    expect(screen.queryByRole("group", { name: "Approval mode" })).toBeNull();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("falls back to full access for an unrecognized stored value", () => {
    globalThis.localStorage.setItem(approvalModeKey("r-1"), "yolo");
    renderPage();
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.full_access.label })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
  });

  it("still remembers the choice when no conversation is open yet", async () => {
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: false, refetch: refetchSessions });
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label }));
    // No session to name, so only the runner-level value is handed over.
    expect(getChatTransport().setApprovalMode).not.toHaveBeenCalledWith("", "ask");
    expect(getChatTransport().setRunnerApprovalMode).toHaveBeenCalledWith("r-1", "ask");
    expect(globalThis.localStorage.getItem(approvalModeKey("r-1"))).toBe("ask");
  });

  it("keeps the mode control usable while a send is in flight", async () => {
    actions.sending = true;
    renderPage();
    expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label })).toBeEnabled();
  });

  it("falls back to full access when storage refuses the read", () => {
    const getItem = vi.spyOn(globalThis.localStorage, "getItem").mockImplementation(() => {
      throw new Error("private mode");
    });
    try {
      renderPage();
      expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.full_access.label })).toHaveAttribute(
        "aria-pressed",
        "true"
      );
    } finally {
      getItem.mockRestore();
    }
  });

  it("never shows a request whose deadline already passed", () => {
    stream.approval = {
      ...approval,
      payload: { ...approval.payload, expires_at: new Date(Date.now() - 1_000).toISOString() },
    };
    renderPage();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("refuses to answer a request when no conversation is open", async () => {
    // A parked request can outlive its session for the render that drops it;
    // there is no session id to answer against, so nothing is sent.
    hooks.useChatSessions.mockReturnValue({ data: [], isPending: false, refetch: refetchSessions });
    stream.approval = approval;
    render(<RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(decide).not.toHaveBeenCalled();
  });

  it("keeps working when storage refuses the write", async () => {
    const storage = globalThis.localStorage;
    const setItem = vi.spyOn(storage, "setItem").mockImplementation(() => {
      throw new Error("private mode");
    });
    try {
      renderPage();
      await userEvent.click(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label }));
      expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label })).toHaveAttribute(
        "aria-pressed",
        "true"
      );
      expect(getChatTransport().setApprovalMode).toHaveBeenCalledWith("open", "ask");
    } finally {
      setItem.mockRestore();
    }
  });
});
