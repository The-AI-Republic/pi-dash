// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chat page integration (NEWFRONT-549, RUN-026/028): the real page, hooks
// and query client against stubbed contracts. Pins the two rules that need
// several moving parts to reproduce — a New chat warms its session once,
// and a create that lands late never overrides an explicit pick or link.
import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ChatSession } from "@pidash/api-client";

const contracts = vi.hoisted(() => ({
  createChatSession: vi.fn(),
  getSttConfig: vi.fn(),
  warmChatSession: vi.fn(),
  getRunner: vi.fn(),
  listChatSessions: vi.fn(),
  getChatSession: vi.fn(),
  listChatMessages: vi.fn(),
}));

const active = vi.hoisted(() => ({ transport: null as unknown }));
vi.mock("./transport.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./transport.js")>();
  return {
    ...original,
    chatTransportForRunner: () => active.transport,
    getChatTransport: () => active.transport,
    openChatSession: (...args: unknown[]) => contracts.createChatSession(...args),
    prepareChatSession: (...args: unknown[]) => contracts.warmChatSession(...args),
    fetchChatRunner: (...args: unknown[]) => contracts.getRunner(...args),
    fetchChatSessions: (...args: unknown[]) => contracts.listChatSessions(...args),
    fetchChatMessages: (...args: unknown[]) => contracts.listChatMessages(...args),
  };
});

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { buildRunnersAreaValue, RunnersAreaProvider } = await import("../area.js");
const { makeRunner, makeSession, testClient } = await import("./harness.js");
const { CHAT_COMPOSER_PLACEHOLDER } = await import("./ChatComposer.js");
const { RunnerChatPage } = await import("./RunnerChatPage.js");

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: ReturnType<typeof testClient>;
  children?: React.ReactNode;
}>;

const area = buildRunnersAreaValue({ workspaceSlug: "acme" }, "ws-id", "allowed");

function Shell({ client, children }: { client: ReturnType<typeof testClient>; children: React.ReactNode }) {
  return (
    <Provider client={client}>
      <RunnersAreaProvider value={area}>{children}</RunnersAreaProvider>
    </Provider>
  );
}

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
  contracts.getSttConfig.mockResolvedValue({
    base_url: "",
    model_name: "",
    has_api_key: false,
    last_verified_at: null,
  });
  active.transport = {
    subscribeChatEvents: vi.fn(() => () => undefined),
    sendChatMessage: vi.fn(),
    cancelChat: vi.fn(),
    closeChat: vi.fn(),
  };
  contracts.getRunner.mockResolvedValue(makeRunner());
  contracts.listChatSessions.mockResolvedValue([]);
  contracts.listChatMessages.mockResolvedValue([]);
  contracts.warmChatSession.mockResolvedValue({ ok: true });
});

afterEach(() => {
  for (const stub of Object.values(contracts)) stub.mockReset();
  active.transport = null;
  resetAppClient();
});

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const closedSession = () => makeSession({ id: "closed", status: "closed", last_message_at: "2026-10-18T12:00:00Z" });

describe("RunnerChatPage integration", () => {
  it("warms the session New chat created exactly once", async () => {
    contracts.listChatSessions.mockResolvedValue([
      makeSession({ id: "open", last_message_at: "2026-10-18T11:00:00Z" }),
    ]);
    contracts.createChatSession.mockResolvedValue(
      makeSession({ id: "fresh", last_message_at: null, created_at: "2026-10-18T13:00:00Z" })
    );
    const client = testClient();
    render(
      <Shell client={client}>
        <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
      </Shell>
    );
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "open"));
    contracts.warmChatSession.mockClear();

    await userEvent.click(screen.getByRole("button", { name: "New chat" }));

    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "fresh"));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(contracts.warmChatSession.mock.calls).toEqual([[expect.anything(), "fresh"]]);
  });

  it("keeps an explicit click when an auto-create resolves afterwards", async () => {
    contracts.listChatSessions.mockResolvedValue([closedSession()]);
    const pending = deferred<ChatSession>();
    contracts.createChatSession.mockReturnValue(pending.promise);
    const client = testClient();
    render(
      <Shell client={client}>
        <RunnerChatPage workspaceSlug="acme" runnerId="r-1" requestedSessionId={null} onSelectSession={vi.fn()} />
      </Shell>
    );
    await waitFor(() => expect(contracts.createChatSession).toHaveBeenCalled());

    const entry = screen.getByTestId("runner-chat-history").querySelector('nav button[type="button"]');
    await userEvent.click(entry as HTMLElement);
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");

    await act(async () => {
      pending.resolve(makeSession({ id: "created", last_message_at: null }));
    });

    await waitFor(() => expect(contracts.listChatMessages).toHaveBeenCalledWith(expect.anything(), "closed"));
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");
    expect(screen.getByPlaceholderText(CHAT_COMPOSER_PLACEHOLDER)).toBeDisabled();
  });

  it("lets a deep link that arrives after a create still select its session", async () => {
    contracts.listChatSessions.mockResolvedValue([closedSession()]);
    contracts.createChatSession.mockResolvedValue(makeSession({ id: "created", last_message_at: null }));
    const client = testClient();
    const page = (requestedSessionId: string | null) => (
      <Shell client={client}>
        <RunnerChatPage
          workspaceSlug="acme"
          runnerId="r-1"
          requestedSessionId={requestedSessionId}
          onSelectSession={vi.fn()}
        />
      </Shell>
    );
    const view = render(page(null));
    await waitFor(() => expect(contracts.listChatMessages).toHaveBeenCalledWith(expect.anything(), "created"));

    view.rerender(page("closed"));

    await waitFor(() => expect(contracts.listChatMessages).toHaveBeenLastCalledWith(expect.anything(), "closed"));
    expect(screen.getByTestId("runner-chat-block-reason")).toHaveTextContent("Session closed");
  });

  it("drops a New chat create that resolves after switching contacts", async () => {
    contracts.listChatSessions.mockResolvedValue([
      makeSession({ id: "open-1", last_message_at: "2026-10-18T11:00:00Z" }),
    ]);
    const pending = deferred<ChatSession>();
    contracts.createChatSession.mockReturnValue(pending.promise);
    const client = testClient();
    const onSelectSession = vi.fn();
    const page = (runnerId: string) => (
      <Shell client={client}>
        <RunnerChatPage
          workspaceSlug="acme"
          runnerId={runnerId}
          requestedSessionId={null}
          onSelectSession={onSelectSession}
        />
      </Shell>
    );
    const view = render(page("r-1"));
    await waitFor(() => expect(contracts.warmChatSession).toHaveBeenCalledWith(expect.anything(), "open-1"));

    await userEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(contracts.createChatSession).toHaveBeenCalledWith(expect.anything(), { workspace: "ws-id", runner: "r-1" });

    view.rerender(page("r-2"));
    await act(async () => {
      pending.resolve(makeSession({ id: "fresh", runner: "r-1", last_message_at: null }));
    });

    await waitFor(() => expect(contracts.listChatMessages).toHaveBeenLastCalledWith(expect.anything(), "open-1"));
    expect(onSelectSession).not.toHaveBeenCalled();
    expect(contracts.listChatMessages.mock.calls.some((call) => call[1] === "fresh")).toBe(false);
    expect(screen.getByRole("button", { name: "New chat" })).toBeEnabled();
  });
});
