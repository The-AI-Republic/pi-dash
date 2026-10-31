// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// @vitest-environment jsdom
// Desktop bootstrap (NEWFRONT-553, RUN-036): the local transport replaces
// the cloud one once, only when the platform offers local chat, and the
// workspace slug comes from the address.
import { afterEach, describe, expect, it, vi } from "vitest";

const platformState = vi.hoisted(() => ({ localChat: undefined as unknown }));
const runtimeState = vi.hoisted(() => ({ account: "u-1", ensure: vi.fn(async () => undefined) }));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return { kind: "desktop", localChat: platformState.localChat };
  },
}));
vi.mock("../runtime/service.js", () => ({
  getRuntimeAccount: () => runtimeState.account,
  ensureChatRuntime: runtimeState.ensure,
}));
vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ baseUrl: "https://api.test" }) }));

const { registerLocalChatTransport, resetLocalChatTransportForTests, workspaceSlugFromPath } =
  await import("./localBootstrap.js");
const { getChatTransport } = await import("./transport.js");
const { approvalModeKey } = await import("./approvalCopy.js");
type LocalChatApi = import("../../../core/platform/index.js").LocalChatApi;

function localApi(): LocalChatApi {
  return {
    listSessions: vi.fn(async () => []),
    createSession: vi.fn(),
    getSession: vi.fn(async () => null),
    listEvents: vi.fn(async () => []),
    appendEvent: vi.fn(),
    setThreadId: vi.fn(),
    warm: vi.fn(),
    send: vi.fn(),
    cancel: vi.fn(),
    close: vi.fn(),
    decide: vi.fn(),
    renameSession: vi.fn(),
    deleteSession: vi.fn(),
    clearHistory: vi.fn(),
    workingDir: vi.fn(),
    subscribe: vi.fn(() => () => undefined),
  };
}

afterEach(() => {
  resetLocalChatTransportForTests();
  platformState.localChat = undefined;
  runtimeState.ensure.mockClear();
});

describe("workspaceSlugFromPath", () => {
  it("reads the first segment, decoding what the router encoded", () => {
    expect(workspaceSlugFromPath("/acme/runners/chat/pidash-builtin")).toBe("acme");
    expect(workspaceSlugFromPath("/acme")).toBe("acme");
    expect(workspaceSlugFromPath("/my%20workspace/runners")).toBe("my workspace");
    expect(workspaceSlugFromPath("/")).toBe("");
    expect(workspaceSlugFromPath("")).toBe("");
  });
});

describe("registerLocalChatTransport", () => {
  it("keeps the cloud transport when the platform has no local chat", () => {
    const before = getChatTransport();
    registerLocalChatTransport();
    expect(getChatTransport()).toBe(before);
  });

  it("installs the local transport once and keeps it there", () => {
    platformState.localChat = localApi();
    const before = getChatTransport();
    registerLocalChatTransport();
    const installed = getChatTransport();
    expect(installed).not.toBe(before);
    registerLocalChatTransport();
    expect(getChatTransport()).toBe(installed);
  });

  it("carries the address's workspace slug on every engine command", async () => {
    const api = localApi();
    vi.mocked(api.getSession).mockResolvedValue({
      id: "s-1",
      title: "",
      workspace: "ws-id",
      project: "pidash-builtin",
      working_dir: "/repo",
      engine_version: "",
      engine_thread_id: null,
      created_at: 0,
      updated_at: 0,
    });
    platformState.localChat = api;
    registerLocalChatTransport();
    const slug = workspaceSlugFromPath(globalThis.location.pathname);
    await getChatTransport().warmChatSession?.("s-1");
    expect(runtimeState.ensure).toHaveBeenCalledTimes(1);
    expect(api.warm).toHaveBeenCalledWith(expect.objectContaining({ workspaceSlug: slug, sessionId: "s-1" }));
    await getChatTransport().cancelChat("s-1", "stop");
    expect(api.cancel).toHaveBeenCalledWith({ workspaceSlug: slug, sessionId: "s-1", reason: "stop" });
  });

  it("builds a transport that reads the signed account at call time", async () => {
    const api = localApi();
    platformState.localChat = api;
    registerLocalChatTransport();
    await getChatTransport().listChatSessions?.("ws-id", "pidash-builtin");
    expect(api.listSessions).toHaveBeenCalledWith("u-1");
    runtimeState.account = "u-2";
    await getChatTransport().listChatSessions?.("ws-id", "pidash-builtin");
    expect(api.listSessions).toHaveBeenLastCalledWith("u-2");
    runtimeState.account = "u-1";
  });

  it("reads the stored approval mode for the runner it is about to warm", async () => {
    const api = localApi();
    vi.mocked(api.getSession).mockResolvedValue({
      id: "s-1",
      title: "",
      workspace: "ws-id",
      project: "pidash-builtin",
      working_dir: "/repo",
      engine_version: "",
      engine_thread_id: null,
      created_at: 1,
      updated_at: 2,
    });
    platformState.localChat = api;
    globalThis.localStorage.setItem(approvalModeKey("pidash-builtin"), "workspace");
    registerLocalChatTransport();
    try {
      await getChatTransport().warmChatSession?.("s-1");
      expect(api.warm).toHaveBeenCalledWith(expect.objectContaining({ mode: "workspace" }));
    } finally {
      globalThis.localStorage.clear();
    }
  });
});
