/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, waitFor } from "@testing-library/react";
import { SWRConfig } from "swr";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { IAgentChatSession, IRunner } from "@pi-dash/types";

// The page is exercised against the real SWR so a remount can hit a warm cache,
// which is the only way the chat page gets `runner` and `sessions` on its very
// first render.
const { transport, getRunnerDetail } = vi.hoisted(() => ({
  getRunnerDetail: vi.fn(),
  transport: {
    listChatSessions: vi.fn(),
    createChatSession: vi.fn(),
    listChatMessages: vi.fn(),
    sendChatMessage: vi.fn(),
    warmChatSession: vi.fn(),
    cancelChat: vi.fn(),
    closeChatSession: vi.fn(),
    // Local-only verbs: their presence is what turns approvals on.
    setApprovalMode: vi.fn(),
    getApprovalMode: vi.fn(),
    decideChatApproval: vi.fn(),
  },
}));

// Transport calls in order, so a test can assert the mode lands before the warm.
const calls: string[] = [];

vi.mock("react-router", () => ({
  useParams: () => ({ workspaceSlug: "acme", runnerId: "runner-1" }),
  useSearchParams: () => [new URLSearchParams(), vi.fn()],
}));

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

vi.mock("@pi-dash/services", () => ({
  getChatTransport: () => transport,
  getRunnerDetail,
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "error" },
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/ui", () => ({
  Badge: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
  Button: ({ children }: { children: React.ReactNode }) => <button type="button">{children}</button>,
}));

vi.mock("@pi-dash/utils", () => ({
  calculateTimeAgo: () => "just now",
  renderFormattedDate: () => "Jan 01, 00:00",
}));

vi.mock("@/hooks/store/use-workspace", () => ({
  useWorkspace: () => ({ currentWorkspace: { id: "ws-1" } }),
}));

vi.mock("@/components/runners/chat/use-agent-chat-events", () => ({
  useAgentChatEvents: () => undefined,
}));

vi.mock("@/components/chat/composer", () => ({ ChatComposer: () => null }));
vi.mock("@/components/chat/container", () => ({
  ChatContainer: ({ children }: { children?: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/components/chat/history-panel", () => ({ ChatHistoryPanel: () => null }));
vi.mock("@/components/chat/message", () => ({ ChatMessage: () => null }));

vi.mock("@/pi-dash-web/components/desktop", () => ({
  ChatApprovalModeSelect: ({ value }: { value: string }) => <span data-testid="approval-mode">{value}</span>,
  ChatApprovalPrompt: () => null,
}));

import RunnerChatPage from "../../app/(all)/[workspaceSlug]/runners/chat/[runnerId]/page";

const STORAGE_KEY = "pidash:chat-approval-mode:runner-1";

const RUNNER = { id: "runner-1", name: "built-in", status: "online" } as IRunner;

const SESSION = {
  id: "session-1",
  status: "open",
  runner: "runner-1",
  last_message_at: "2026-01-01T00:00:00Z",
  created_at: "2026-01-01T00:00:00Z",
  active_message_id: null,
  active_turn_id: null,
} as unknown as IAgentChatSession;

function renderPage(cache: Map<string, unknown>) {
  return render(
    <SWRConfig value={{ provider: () => cache as never, dedupingInterval: 0 }}>
      <RunnerChatPage />
    </SWRConfig>
  );
}

describe("RunnerChatPage — approval mode on warm", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    calls.length = 0;
    window.localStorage.clear();
    getRunnerDetail.mockResolvedValue(RUNNER);
    transport.listChatSessions.mockResolvedValue([SESSION]);
    transport.listChatMessages.mockResolvedValue([]);
    transport.setApprovalMode.mockImplementation((_sessionId: string, mode: string) => {
      calls.push(`mode:${mode}`);
    });
    transport.warmChatSession.mockImplementation(async () => {
      calls.push("warm");
      return { ok: true };
    });
  });

  it("warms with the saved mode on a cold mount", async () => {
    window.localStorage.setItem(STORAGE_KEY, "ask");

    renderPage(new Map());

    await waitFor(() => expect(transport.warmChatSession).toHaveBeenCalledWith("session-1"));
    expect(calls).toEqual(["mode:ask", "warm"]);
  });

  it("warms with the saved mode when remounted on a warm SWR cache", async () => {
    window.localStorage.setItem(STORAGE_KEY, "ask");
    const cache = new Map<string, unknown>();

    // First visit fills the cache; navigating away unmounts the page.
    const first = renderPage(cache);
    await waitFor(() => expect(transport.warmChatSession).toHaveBeenCalledTimes(1));
    first.unmount();
    calls.length = 0;
    transport.warmChatSession.mockClear();

    // Coming back: runner + sessions are available on the very first render,
    // so the warm runs in the same commit that loads the saved mode.
    renderPage(cache);

    await waitFor(() => expect(transport.warmChatSession).toHaveBeenCalledWith("session-1"));
    expect(calls).toEqual(["mode:ask", "warm"]);
  });
});
