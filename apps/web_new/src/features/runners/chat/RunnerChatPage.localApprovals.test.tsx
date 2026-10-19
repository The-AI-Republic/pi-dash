// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop chat, end to end through the page (NEWFRONT-553, RUN-034/035):
// the local engine's frames reach the prompt, the decision is answered
// through the native verb, the choice is remembered per runner, and a
// terminal frame takes a parked request away.
import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({
  getSttConfig: vi.fn(),
  getAgentProfile: vi.fn(),
}));
const platformState = vi.hoisted(() => ({
  api: null as unknown,
  account: "u-1",
  ensure: vi.fn(async () => undefined),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    kind: "desktop",
    get localChat() {
      return platformState.api;
    },
  },
}));
vi.mock("../runtime/service.js", () => ({
  getRuntimeAccount: () => platformState.account,
  ensureChatRuntime: platformState.ensure,
}));
vi.mock("../../../shared/dictation/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/dictation/index.js")>();
  return { ...original, useDictationConfig: () => ({ data: undefined, isPending: false }) };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/index.js");
const { buildRunnersAreaValue, RunnersAreaProvider } = await import("../area.js");
const { testClient } = await import("./harness.js");
const { registerLocalChatTransport, resetLocalChatTransportForTests } = await import("./localBootstrap.js");
const { APPROVAL_MODE_COPY, approvalModeKey } = await import("./approvalCopy.js");
const { RunnerChatPage } = await import("./RunnerChatPage.js");

type LocalChatApi = import("../../../core/platform/index.js").LocalChatApi;
type LocalChatFrame = import("../../../core/platform/index.js").LocalChatFrame;

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: ReturnType<typeof testClient>;
  children?: React.ReactNode;
}>;

const area = buildRunnersAreaValue({ workspaceSlug: "acme" }, "ws-id", "allowed");

let emit: (frame: LocalChatFrame) => void = () => undefined;
let failStream: (event: { chat_session_id: string; message: string }) => void = () => undefined;
let subscribed = false;

function localApi(): LocalChatApi {
  return {
    listSessions: vi.fn(async () => [
      {
        id: "s-1",
        title: "",
        workspace: "ws-id",
        project: "pidash-builtin",
        working_dir: "/repo",
        engine_version: "",
        engine_thread_id: null,
        created_at: 1,
        updated_at: 2,
      },
    ]),
    createSession: vi.fn(async () => {
      throw new Error("not used");
    }),
    getSession: vi.fn(async () => ({
      id: "s-1",
      title: "",
      workspace: "ws-id",
      project: "pidash-builtin",
      working_dir: "/repo",
      engine_version: "",
      engine_thread_id: null,
      created_at: 1,
      updated_at: 2,
    })),
    listEvents: vi.fn(async () => []),
    appendEvent: vi.fn(async (_account, sessionId, input) => ({
      id: "e-1",
      session_id: sessionId,
      seq: 1,
      role: input.role,
      content: input.content,
      tool_calls: null,
      approval_decision: null,
      created_at: 2,
    })),
    setThreadId: vi.fn(async () => undefined),
    warm: vi.fn(async () => undefined),
    send: vi.fn(async () => undefined),
    cancel: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    decide: vi.fn(async () => undefined),
    subscribe: vi.fn((handlers) => {
      emit = handlers.onFrame;
      failStream = handlers.onError;
      subscribed = true;
      return () => undefined;
    }),
  };
}

function renderPage() {
  return render(
    <Provider client={testClient()}>
      <RunnersAreaProvider value={area}>
        <RunnerChatPage
          workspaceSlug="acme"
          runnerId="pidash-builtin"
          requestedSessionId="s-1"
          onSelectSession={vi.fn()}
        />
      </RunnersAreaProvider>
    </Provider>
  );
}

beforeEach(() => {
  subscribed = false;
  globalThis.history.replaceState({}, "", "/acme/runners/chat/pidash-builtin");
  createAppClient("https://api.test", {} as never);
  contracts.getSttConfig.mockResolvedValue({
    base_url: "",
    model_name: "",
    has_api_key: false,
    last_verified_at: null,
  });
  contracts.getAgentProfile.mockResolvedValue({
    managed_runner_enabled: true,
    lane: "local",
    base_url: "",
    model: "m",
    available: true,
    reason_code: "",
    graceful_stop_seconds: 5,
  });
  platformState.api = localApi();
  registerLocalChatTransport();
});

afterEach(() => {
  resetLocalChatTransportForTests();
  resetAppClient();
  globalThis.localStorage.clear();
  contracts.getAgentProfile.mockReset();
});

function approvalFrame(): LocalChatFrame {
  return {
    result: "chat_approval_request",
    data: {
      chat_session_id: "s-1",
      local_approval_id: "a-1",
      kind: "command_execution",
      payload: { command: "npm test" },
      reason: "verify the change",
    },
  };
}

describe("RunnerChatPage local approvals", () => {
  it("shows a parked request and answers it through the native verb", async () => {
    const api = platformState.api as LocalChatApi;
    renderPage();
    await waitFor(() => expect(subscribed).toBe(true));
    await act(async () => emit(approvalFrame()));

    const prompt = await screen.findByRole("alertdialog", { name: "Approval requested" });
    expect(prompt).toHaveTextContent("npm test");

    await userEvent.click(screen.getByRole("button", { name: "Always allow this session" }));
    expect(api.decide).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      sessionId: "s-1",
      localApprovalId: "a-1",
      decision: "accept_for_session",
    });
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  });

  it("clears a parked request when its turn ends and keeps it on a failure", async () => {
    renderPage();
    await waitFor(() => expect(subscribed).toBe(true));
    await act(async () => emit(approvalFrame()));
    await screen.findByRole("alertdialog", { name: "Approval requested" });

    await act(async () => emit({ result: "chat_closed", data: { chat_session_id: "s-1" } }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());

    await act(async () => emit(approvalFrame()));
    await screen.findByRole("alertdialog", { name: "Approval requested" });
    await act(async () =>
      emit({ result: "chat_failed", data: { chat_session_id: "s-1", code: "boom", detail: "engine died" } })
    );
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  });

  it("shows a stream failure named by the native error channel", async () => {
    renderPage();
    await waitFor(() => expect(subscribed).toBe(true));
    await act(async () => failStream({ chat_session_id: "s-1", message: "socket closed" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("socket closed");
  });

  it("remembers the mode the user picks for the next thread", async () => {
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: APPROVAL_MODE_COPY.ask.label }));
    expect(globalThis.localStorage.getItem(approvalModeKey("pidash-builtin"))).toBe("ask");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: APPROVAL_MODE_COPY.ask.label })).toHaveAttribute("aria-pressed", "true")
    );
  });
});
