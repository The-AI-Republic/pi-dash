// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    getAgentProfile: vi.fn(),
    postAgentToken: vi.fn(),
    createDesktopEnroll: vi.fn(),
    deleteDesktopEnroll: vi.fn(),
    getProject: vi.fn(),
    listProjects: vi.fn(),
    getIssue: vi.fn(),
  };
});

import {
  ApiError,
  createDesktopEnroll,
  deleteDesktopEnroll,
  getAgentProfile,
  getIssue,
  getProject,
  listProjects,
  postAgentToken,
  type AgentProfile,
  type AgentToken,
  type ApiClient,
  type DesktopEnrollResult,
  type Issue,
  type Project,
  type ProjectDetail,
} from "@pidash/api-client";

import type { AgentRuntimeApi } from "../../../core/platform/index.js";
import {
  connectProject,
  disposeRuntimeForSignOut,
  EngineUnavailableError,
  ensureChatRuntime,
  getRuntimeAccount,
  prepareAgentRun,
  refreshRuntime,
  resetRuntimeServiceForTests,
  resumeRuntime,
} from "./service.js";

const FAR_FUTURE = "2999-01-01T00:00:00Z";
const CACHE_KEY = "pidash-managed-workspaces:u1";

const client = { baseUrl: "https://cloud.test" } as unknown as ApiClient;

function fakeRuntime(): AgentRuntimeApi {
  return {
    isAvailable: vi.fn(async () => true),
    checkHost: vi.fn(async () => ({ runnerPresent: true, enginePresent: true, hostLabel: "h" })),
    writeEngineConfig: vi.fn(async () => undefined),
    writeModelToken: vi.fn(async () => undefined),
    bootstrapMachine: vi.fn(async () => undefined),
    enrollProject: vi.fn(async () => undefined),
    startDaemon: vi.fn(async () => undefined),
    stopDaemon: vi.fn(async () => undefined),
    signOutNative: vi.fn(async () => undefined),
    saveApprovalModes: vi.fn(async () => undefined),
    loadApprovalModes: vi.fn(async () => ({})),
  };
}

function profile(overrides: Partial<AgentProfile> = {}): AgentProfile {
  return {
    managed_runner_enabled: true,
    lane: "lane",
    base_url: "https://models.test",
    model: "model",
    available: true,
    reason_code: "",
    graceful_stop_seconds: 5,
    ...overrides,
  };
}

function token(expiresAt: string = FAR_FUTURE): AgentToken {
  return { token: "tok", expires_at: expiresAt };
}

function enrollResult(): DesktopEnrollResult {
  return {
    dev_machine_id: "dm-1",
    machine_token: "mt",
    workspace_slug: "ws",
    managed_runner_enabled: true,
    graceful_stop_seconds: 5,
  };
}

function projectDetail(overrides: Partial<ProjectDetail> = {}): ProjectDetail {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    name: "Acme",
    identifier: "ACME",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: "323e4567-e89b-12d3-a456-426614174002",
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function projectRow(overrides: Partial<Project> = {}): Project {
  return { ...projectDetail(overrides), intake_count: 0 };
}

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    name: "Fix login",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: "223e4567-e89b-12d3-a456-426614174001",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function apiError(status: number): ApiError {
  return new ApiError({ status, code: "http", message: `status ${status}` });
}

function resume(runtime: AgentRuntimeApi, userId = "u1"): void {
  resumeRuntime(userId, { client, runtime });
}

/** Working local-storage double (this jsdom setup exposes none). */
function memoryStorage(): Storage {
  const entries = new Map<string, string>();
  return {
    get length() {
      return entries.size;
    },
    key: (index: number) => [...entries.keys()][index] ?? null,
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => {
      entries.set(key, value);
    },
    removeItem: (key: string) => {
      entries.delete(key);
    },
    clear: () => {
      entries.clear();
    },
  } as Storage;
}

beforeEach(() => {
  resetRuntimeServiceForTests();
  vi.stubGlobal("localStorage", memoryStorage());
  globalThis.sessionStorage.clear();
  vi.clearAllMocks();
  vi.mocked(getAgentProfile).mockResolvedValue(profile());
  vi.mocked(postAgentToken).mockResolvedValue(token());
  vi.mocked(createDesktopEnroll).mockResolvedValue(enrollResult());
  vi.mocked(deleteDesktopEnroll).mockResolvedValue(undefined);
  vi.mocked(getProject).mockResolvedValue(projectDetail());
  vi.mocked(listProjects).mockResolvedValue([]);
  vi.mocked(getIssue).mockResolvedValue(issue());
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("resumeRuntime", () => {
  it("is inert without the desktop capability", () => {
    resumeRuntime("u1", { client });
    expect(getRuntimeAccount()).toBe("");
  });

  it("resumes for a new account and restores its approval modes", () => {
    const runtime = fakeRuntime();
    resume(runtime);
    expect(getRuntimeAccount()).toBe("u1");
    expect(runtime.loadApprovalModes).toHaveBeenCalledWith("u1");
  });

  it("ignores a repeat resume for the same account", () => {
    const runtime = fakeRuntime();
    resume(runtime);
    resume(runtime);
    expect(runtime.loadApprovalModes).toHaveBeenCalledTimes(1);
  });

  it("clears the stopped flag without an account", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    resumeRuntime(undefined, { client, runtime });
    expect(getRuntimeAccount()).toBe("u1");
  });

  it("restores the enrollment cache so a reload reuses the daemon", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
    resetRuntimeServiceForTests();
    const reloaded = fakeRuntime();
    resume(reloaded);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime: reloaded });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
    expect(getRuntimeAccount()).toBe("u1");
  });

  it("swallows a failing mode restore", async () => {
    const runtime = fakeRuntime();
    vi.mocked(runtime.loadApprovalModes).mockRejectedValue(new Error("host gone"));
    resume(runtime);
    expect(getRuntimeAccount()).toBe("u1");
    await Promise.resolve();
  });

  it("drops a corrupt enrollment cache", async () => {
    const runtime = fakeRuntime();
    globalThis.sessionStorage.setItem(CACHE_KEY, "{nope");
    resume(runtime);
    expect(globalThis.sessionStorage.getItem(CACHE_KEY)).toBeNull();
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
  });

  it("ignores a cache with a non-string host label", async () => {
    const runtime = fakeRuntime();
    globalThis.sessionStorage.setItem(CACHE_KEY, JSON.stringify({ hostLabel: 42, workspaces: ["ws"] }));
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
  });

  it("ignores a cache with a non-list workspace set", async () => {
    const runtime = fakeRuntime();
    globalThis.sessionStorage.setItem(CACHE_KEY, JSON.stringify({ hostLabel: "h", workspaces: "ws" }));
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
  });

  it("keeps workspaces while skipping a null chat-project map", async () => {
    const runtime = fakeRuntime();
    globalThis.sessionStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ hostLabel: "h", workspaces: ["ws"], chatProjects: null })
    );
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).not.toHaveBeenCalled();
  });

  it("keeps workspaces while skipping a malformed chat-project map", async () => {
    const runtime = fakeRuntime();
    globalThis.sessionStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ hostLabel: "h", workspaces: ["ws"], chatProjects: "ACME" })
    );
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).not.toHaveBeenCalled();
  });

  it("skips non-string entries while restoring", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow({ identifier: "OTHER" })]);
    globalThis.sessionStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ hostLabel: "h", workspaces: ["ws", 42], chatProjects: { ws: "ACME", other: 7 } })
    );
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).not.toHaveBeenCalled();
    await ensureChatRuntime({ workspaceSlug: "other", client, runtime });
    expect(listProjects).toHaveBeenCalled();
    expect(runtime.enrollProject).toHaveBeenLastCalledWith(expect.objectContaining({ project: "OTHER" }));
  });

  it("survives an unreadable store", () => {
    const runtime = fakeRuntime();
    const spy = vi.spyOn(globalThis.sessionStorage, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    try {
      resume(runtime);
    } finally {
      spy.mockRestore();
    }
    expect(getRuntimeAccount()).toBe("u1");
  });
});

describe("connectProject", () => {
  it("is inert without the desktop capability", async () => {
    await connectProject({ workspaceSlug: "ws", projectId: "p", client });
    expect(getAgentProfile).not.toHaveBeenCalled();
  });

  it("connects a project end to end", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(getAgentProfile).toHaveBeenCalledWith(client);
    expect(runtime.writeEngineConfig).toHaveBeenCalledWith({
      account: "u1",
      profile: { available: true, base_url: "https://models.test", model: "model", reason_code: "" },
    });
    expect(postAgentToken).toHaveBeenCalledWith(client);
    expect(runtime.writeModelToken).toHaveBeenCalledWith("tok");
    expect(createDesktopEnroll).toHaveBeenCalledWith(client, { workspaceSlug: "ws", hostLabel: "h" });
    expect(runtime.bootstrapMachine).toHaveBeenCalledWith({
      cloudUrl: "https://cloud.test",
      workspace: "ws",
      machineToken: "mt",
      devMachineId: "dm-1",
    });
    expect(getProject).toHaveBeenCalledWith(client, "ws", "p");
    expect(runtime.enrollProject).toHaveBeenCalledWith({
      account: "u1",
      workspace: "ws",
      project: "ACME",
      hostLabel: "h",
    });
    expect(runtime.startDaemon).toHaveBeenCalledWith({ account: "u1", workspace: "ws" });
    expect(JSON.parse(globalThis.sessionStorage.getItem(CACHE_KEY) ?? "null")).toEqual({
      hostLabel: "h",
      workspaces: ["ws"],
      chatProjects: { ws: "ACME" },
    });
  });

  it("enrolls each workspace once per session", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
    expect(runtime.bootstrapMachine).toHaveBeenCalledTimes(1);
    expect(runtime.startDaemon).toHaveBeenCalledTimes(2);
  });

  it("forgets enrollments when the host label changes", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    vi.mocked(runtime.checkHost).mockResolvedValue({ runnerPresent: true, enginePresent: true, hostLabel: "h2" });
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    expect(createDesktopEnroll).toHaveBeenCalledTimes(2);
    expect(createDesktopEnroll).toHaveBeenLastCalledWith(client, { workspaceSlug: "ws", hostLabel: "h2" });
  });

  it.each([[{ runnerPresent: false, enginePresent: true }], [{ runnerPresent: true, enginePresent: false }]])(
    "asks for a reinstall when binaries are missing (%o)",
    async (missing) => {
      const runtime = fakeRuntime();
      vi.mocked(runtime.checkHost).mockResolvedValue({ ...missing, hostLabel: "h" });
      resume(runtime);
      await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow(
        "Reinstall"
      );
    }
  );

  it("stops the daemon and reports the plain-language reason when the server refuses the agent", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getAgentProfile).mockResolvedValue(profile({ available: false, reason_code: "managed_runner_disabled" }));
    resume(runtime);
    const failure = await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime }).catch(
      (error: unknown) => error
    );
    expect(failure).toBeInstanceOf(EngineUnavailableError);
    expect((failure as EngineUnavailableError).code).toBe("managed_runner_disabled");
    expect((failure as Error).message).toContain("switched off");
    expect((failure as Error).message).not.toContain("managed_runner_disabled");
    expect(runtime.stopDaemon).toHaveBeenCalledWith();
    expect(runtime.writeModelToken).not.toHaveBeenCalled();
  });

  it("surfaces an unmapped refusal code unchanged", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getAgentProfile).mockResolvedValue(profile({ available: false, reason_code: "future_refusal" }));
    resume(runtime);
    const failure = await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime }).catch(
      (error: unknown) => error
    );
    expect((failure as Error).message).toBe("future_refusal");
  });

  it.each([
    ["empty token", { token: "", expires_at: FAR_FUTURE }],
    ["malformed expiry", { token: "tok", expires_at: "not-a-date" }],
    ["past expiry", { token: "tok", expires_at: "2000-01-01T00:00:00Z" }],
  ])("rejects an unusable credential (%s)", async (_label, credential) => {
    const runtime = fakeRuntime();
    vi.mocked(postAgentToken).mockResolvedValue(credential);
    resume(runtime);
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow(
      "session expired"
    );
    expect(runtime.writeModelToken).not.toHaveBeenCalled();
  });

  it("skips renewal while the credential has life left", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await connectProject({ workspaceSlug: "ws", projectId: "other", client, runtime });
    expect(postAgentToken).toHaveBeenCalledTimes(1);
  });

  it("refuses to connect while signed out", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow(
      "signed out"
    );
  });

  it("abandons a connection superseded by a newer sign-in", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    const pending = connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    const disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    resume(runtime);
    await expect(pending).rejects.toThrow("signed out");
    await disposal;
  });

  it("stops quietly when signed out mid-connection", async () => {
    const runtime = fakeRuntime();
    let disposal: Promise<void> | undefined;
    vi.mocked(runtime.writeModelToken).mockImplementation(() => {
      disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
      return Promise.resolve();
    });
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposal;
    expect(runtime.startDaemon).not.toHaveBeenCalled();
  });

  it("stops quietly when signed out before the daemon start", async () => {
    const runtime = fakeRuntime();
    let disposal: Promise<void> | undefined;
    vi.mocked(runtime.enrollProject).mockImplementation(() => {
      disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
      return Promise.resolve();
    });
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposal;
    expect(runtime.startDaemon).not.toHaveBeenCalled();
  });

  it("needs a resumed account for engine commands", async () => {
    const runtime = fakeRuntime();
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow(
      "signed out"
    );
  });

  it("tears the runtime down on a 401 and rethrows", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getAgentProfile).mockRejectedValue(apiError(401));
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toMatchObject({
      status: 401,
    });
    expect(runtime.signOutNative).toHaveBeenCalledWith();
    expect(globalThis.sessionStorage.getItem(CACHE_KEY)).toBeNull();
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow(
      "signed out"
    );
  });

  it("still reports the 401 when the native sign-out fails", async () => {
    const runtime = fakeRuntime();
    vi.mocked(runtime.signOutNative).mockRejectedValue(new Error("host gone"));
    resume(runtime);
    vi.mocked(getAgentProfile).mockRejectedValue(apiError(401));
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toMatchObject({
      status: 401,
    });
  });

  it("leaves the runtime alone on other API failures", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getAgentProfile).mockRejectedValue(apiError(500));
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toMatchObject({
      status: 500,
    });
    expect(runtime.signOutNative).not.toHaveBeenCalled();
  });

  it("leaves the runtime alone on transport failures", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    vi.mocked(getAgentProfile).mockRejectedValue(new TypeError("down"));
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow("down");
    expect(runtime.signOutNative).not.toHaveBeenCalled();
  });

  it("survives an unwritable store", async () => {
    const runtime = fakeRuntime();
    const setItem = vi.fn(() => {
      throw new Error("denied");
    });
    vi.stubGlobal("sessionStorage", { getItem: () => null, setItem, removeItem: () => undefined });
    try {
      resume(runtime);
      await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    } finally {
      vi.unstubAllGlobals();
    }
    expect(setItem).toHaveBeenCalled();
    expect(runtime.startDaemon).toHaveBeenCalled();
  });
});

describe("ensureChatRuntime", () => {
  it("is inert without the desktop capability", async () => {
    await ensureChatRuntime({ workspaceSlug: "ws", client });
    expect(listProjects).not.toHaveBeenCalled();
  });

  it("enrolls chat against the workspace default project", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([
      projectRow({ identifier: "OLD", created_at: "2024-01-01T00:00:00Z" }),
      projectRow({ identifier: "NEW", created_at: "2025-01-01T00:00:00Z", is_default: true }),
    ]);
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(runtime.enrollProject).toHaveBeenCalledWith({
      account: "u1",
      workspace: "ws",
      project: "NEW",
      hostLabel: "h",
    });
  });

  it("enrolls chat against the oldest project without a default", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([
      projectRow({ identifier: "NEW", created_at: "2025-06-01T00:00:00Z" }),
      projectRow({ identifier: "OLD", created_at: "2024-01-01T00:00:00Z" }),
      projectRow({ identifier: "MID", created_at: "2024-06-01T00:00:00Z" }),
    ]);
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(runtime.enrollProject).toHaveBeenCalledWith({
      account: "u1",
      workspace: "ws",
      project: "OLD",
      hostLabel: "h",
    });
  });

  it("prefers live projects but accepts archived ones when alone", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([
      projectRow({ identifier: "LIVE", created_at: "2025-01-01T00:00:00Z" }),
      projectRow({ identifier: "OLD", created_at: "2024-01-01T00:00:00Z", archived_at: "2025-01-01T00:00:00Z" }),
    ]);
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(runtime.enrollProject).toHaveBeenCalledWith(expect.objectContaining({ project: "LIVE" }));
    vi.mocked(listProjects).mockResolvedValue([
      projectRow({ identifier: "OLD", created_at: "2024-01-01T00:00:00Z", archived_at: "2025-01-01T00:00:00Z" }),
    ]);
    await ensureChatRuntime({ workspaceSlug: "ws2", client, runtime });
    expect(runtime.enrollProject).toHaveBeenCalledWith(expect.objectContaining({ workspace: "ws2", project: "OLD" }));
  });

  it("asks for a project when the workspace has none", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([]);
    resume(runtime);
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).rejects.toThrow("needs a project");
  });

  it("rides on a project the user connected themselves", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(listProjects).not.toHaveBeenCalled();
    expect(runtime.enrollProject).toHaveBeenLastCalledWith(expect.objectContaining({ project: "ACME" }));
  });

  it("reuses a ready workspace without re-enrolling", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow()]);
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(runtime.checkHost).toHaveBeenCalledTimes(1);
    expect(createDesktopEnroll).toHaveBeenCalledTimes(1);
    expect(postAgentToken).toHaveBeenCalledTimes(1);
    expect(runtime.startDaemon).toHaveBeenCalledTimes(2);
  });

  it("renews an expiring credential on the fast path", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow()]);
    vi.mocked(postAgentToken).mockResolvedValue(token(new Date(Date.now() + 60_000).toISOString()));
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(postAgentToken).toHaveBeenCalledTimes(2);
    expect(runtime.startDaemon).toHaveBeenCalledTimes(2);
  });

  it("refuses while signed out", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    await expect(ensureChatRuntime({ workspaceSlug: "ws", client, runtime })).rejects.toThrow("signed out");
  });

  it("stops quietly when signed out mid-sequence", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow()]);
    let disposal: Promise<void> | undefined;
    vi.mocked(runtime.writeModelToken).mockImplementation(() => {
      disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
      return Promise.resolve();
    });
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    await disposal;
    expect(listProjects).not.toHaveBeenCalled();
    expect(runtime.startDaemon).not.toHaveBeenCalled();
  });

  it("stops quietly when signed out before the daemon start", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow()]);
    let disposal: Promise<void> | undefined;
    vi.mocked(runtime.enrollProject).mockImplementation(() => {
      disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
      return Promise.resolve();
    });
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    await disposal;
    expect(runtime.startDaemon).not.toHaveBeenCalled();
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(runtime.checkHost).toHaveBeenCalledTimes(2);
  });

  it("stops quietly when signed out on the fast path", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow()]);
    vi.mocked(postAgentToken).mockResolvedValue(token(new Date(Date.now() + 60_000).toISOString()));
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    let disposal: Promise<void> | undefined;
    vi.mocked(runtime.writeModelToken).mockImplementation(() => {
      disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
      return Promise.resolve();
    });
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    await disposal;
    expect(runtime.startDaemon).toHaveBeenCalledTimes(1);
  });
});

describe("prepareAgentRun", () => {
  const fastPoll = { maxAttempts: 3, intervalMs: 0, sleep: vi.fn(async () => undefined) };

  it("is inert without the desktop capability", async () => {
    await prepareAgentRun({ target: { workspaceSlug: "ws", projectId: "p", issueId: "i" }, client });
    expect(getIssue).not.toHaveBeenCalled();
  });

  it("does nothing when the issue pins another executor", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "local_runner" }));
    resume(runtime);
    await prepareAgentRun({ target: { workspaceSlug: "ws", projectId: "p", issueId: "i" }, client, runtime });
    expect(runtime.checkHost).not.toHaveBeenCalled();
  });

  it("prepares when the issue pins the bundled agent", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "managed_runner" }));
    vi.mocked(getProject).mockResolvedValue(
      projectDetail({
        agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }],
      })
    );
    resume(runtime);
    await prepareAgentRun({
      target: { workspaceSlug: "ws", projectId: "p", issueId: "i" },
      client,
      runtime,
      heartbeatPoll: fastPoll,
    });
    expect(runtime.startDaemon).toHaveBeenCalled();
  });

  it("falls back to the project default without a pin", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getProject).mockResolvedValue(
      projectDetail({
        default_agent_executor: "managed_runner",
        agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }],
      })
    );
    resume(runtime);
    await prepareAgentRun({
      target: { workspaceSlug: "ws", projectId: "p", issueId: "i" },
      client,
      runtime,
      heartbeatPoll: fastPoll,
    });
    expect(runtime.startDaemon).toHaveBeenCalled();
  });

  it("does nothing when neither pin nor default names the bundled agent", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await prepareAgentRun({ target: { workspaceSlug: "ws", projectId: "p", issueId: "i" }, client, runtime });
    expect(runtime.checkHost).not.toHaveBeenCalled();
    expect(vi.mocked(getIssue)).toHaveBeenCalled();
  });

  it("waits for the first heartbeat before returning", async () => {
    const runtime = fakeRuntime();
    const sleep = vi.fn(async () => undefined);
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "managed_runner" }));
    vi.mocked(getProject)
      .mockResolvedValueOnce(projectDetail())
      .mockResolvedValueOnce(projectDetail())
      .mockResolvedValueOnce(
        projectDetail({ agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }] })
      )
      .mockResolvedValueOnce(
        projectDetail({ agent_executor_options: [{ kind: "managed_runner", available: false, reason_code: "x" }] })
      )
      .mockResolvedValue(
        projectDetail({ agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }] })
      );
    resume(runtime);
    await prepareAgentRun({
      target: { workspaceSlug: "ws", projectId: "p", issueId: "i" },
      client,
      runtime,
      heartbeatPoll: { maxAttempts: 5, intervalMs: 0, sleep },
    });
    expect(sleep).toHaveBeenCalledTimes(2);
  });

  it("uses the default heartbeat wait when none is given", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "managed_runner" }));
    vi.mocked(getProject).mockResolvedValue(
      projectDetail({
        agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }],
      })
    );
    resume(runtime);
    await prepareAgentRun({ target: { workspaceSlug: "ws", projectId: "p", issueId: "i" }, client, runtime });
    expect(runtime.startDaemon).toHaveBeenCalled();
  });

  it("sleeps between heartbeat polls on the default wait", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "managed_runner" }));
    vi.mocked(getProject)
      .mockResolvedValueOnce(projectDetail())
      .mockResolvedValueOnce(projectDetail())
      .mockResolvedValueOnce(projectDetail())
      .mockResolvedValue(
        projectDetail({ agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }] })
      );
    resume(runtime);
    await prepareAgentRun({ target: { workspaceSlug: "ws", projectId: "p", issueId: "i" }, client, runtime });
    expect(runtime.startDaemon).toHaveBeenCalled();
  }, 10000);

  it("asks for a retry when the daemon never appears", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "managed_runner" }));
    resume(runtime);
    await expect(
      prepareAgentRun({
        target: { workspaceSlug: "ws", projectId: "p", issueId: "i" },
        client,
        runtime,
        heartbeatPoll: fastPoll,
      })
    ).rejects.toThrow("still connecting");
  });

  it("aborts the wait when signed out", async () => {
    const runtime = fakeRuntime();
    vi.mocked(getIssue).mockResolvedValue(issue({ agent_executor: "managed_runner" }));
    resume(runtime);
    const sleep = vi.fn(async () => {
      await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    });
    await expect(
      prepareAgentRun({
        target: { workspaceSlug: "ws", projectId: "p", issueId: "i" },
        client,
        runtime,
        heartbeatPoll: { maxAttempts: 3, intervalMs: 0, sleep },
      })
    ).rejects.toThrow("signed out");
  });
});

describe("refreshRuntime", () => {
  it("is inert without the desktop capability", async () => {
    await refreshRuntime({ client });
    expect(getAgentProfile).not.toHaveBeenCalled();
  });

  it("does nothing while signed out or without enrolled workspaces", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await refreshRuntime({ client, runtime });
    expect(getAgentProfile).not.toHaveBeenCalled();
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    vi.mocked(getAgentProfile).mockClear();
    await refreshRuntime({ client, runtime });
    expect(getAgentProfile).not.toHaveBeenCalled();
  });

  it("refreshes the setup and restarts every workspace daemon", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await connectProject({ workspaceSlug: "ws2", projectId: "p", client, runtime });
    vi.mocked(getAgentProfile).mockClear();
    vi.mocked(runtime.startDaemon).mockClear();
    await refreshRuntime({ client, runtime });
    expect(getAgentProfile).toHaveBeenCalledTimes(1);
    expect(postAgentToken).toHaveBeenCalledTimes(1);
    expect(runtime.startDaemon).toHaveBeenCalledTimes(2);
  });

  it("renews an expiring credential on refresh", async () => {
    const runtime = fakeRuntime();
    vi.mocked(postAgentToken).mockResolvedValue(token(new Date(Date.now() + 60_000).toISOString()));
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await refreshRuntime({ client, runtime });
    expect(postAgentToken).toHaveBeenCalledTimes(2);
  });

  it("drops readiness and rethrows when the refresh fails", async () => {
    const runtime = fakeRuntime();
    vi.mocked(listProjects).mockResolvedValue([projectRow()]);
    resume(runtime);
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    vi.mocked(getAgentProfile).mockRejectedValue(apiError(500));
    await expect(refreshRuntime({ client, runtime })).rejects.toMatchObject({ status: 500 });
    expect(runtime.stopDaemon).not.toHaveBeenCalled();
    vi.mocked(getAgentProfile).mockResolvedValue(profile());
    await ensureChatRuntime({ workspaceSlug: "ws", client, runtime });
    expect(runtime.checkHost).toHaveBeenCalledTimes(2);
  });

  it("stops the daemon when the credential expired mid-refresh", async () => {
    const runtime = fakeRuntime();
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
      vi.mocked(postAgentToken).mockResolvedValue(token("2026-01-01T00:01:00Z"));
      resume(runtime);
      await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
      vi.setSystemTime(new Date("2026-01-01T00:02:00Z"));
      vi.mocked(getAgentProfile).mockRejectedValue(apiError(500));
      await expect(refreshRuntime({ client, runtime })).rejects.toMatchObject({ status: 500 });
      expect(runtime.stopDaemon).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops quietly when signed out before the refresh runs", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    vi.mocked(getAgentProfile).mockClear();
    const pending = refreshRuntime({ client, runtime });
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    await pending;
    expect(getAgentProfile).not.toHaveBeenCalled();
  });

  it("stops quietly when signed out mid-refresh", async () => {
    const runtime = fakeRuntime();
    vi.mocked(postAgentToken).mockResolvedValue(token(new Date(Date.now() + 60_000).toISOString()));
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    let disposal: Promise<void> | undefined;
    vi.mocked(runtime.writeModelToken).mockImplementation(() => {
      disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
      return Promise.resolve();
    });
    vi.mocked(runtime.startDaemon).mockClear();
    vi.mocked(getAgentProfile).mockClear();
    await refreshRuntime({ client, runtime });
    await disposal;
    expect(getAgentProfile).toHaveBeenCalled();
    expect(runtime.startDaemon).not.toHaveBeenCalled();
  });
});

describe("disposeRuntimeForSignOut", () => {
  it("is inert without the desktop capability", async () => {
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client });
    expect(deleteDesktopEnroll).not.toHaveBeenCalled();
  });

  it("tears the runtime down in two passes", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    expect(runtime.signOutNative).toHaveBeenNthCalledWith(1, undefined);
    expect(deleteDesktopEnroll).toHaveBeenCalledWith(client, "h");
    expect(runtime.signOutNative).toHaveBeenNthCalledWith(2);
    expect(globalThis.sessionStorage.getItem(CACHE_KEY)).toBeNull();
    await expect(connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime })).rejects.toThrow(
      "signed out"
    );
  });

  it("passes history deletion only when the dialog chose it", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: true }, client, runtime });
    expect(runtime.signOutNative).toHaveBeenNthCalledWith(1, { deleteChatHistoryAccount: "u1" });
  });

  it("keeps history without a resumed account", async () => {
    const runtime = fakeRuntime();
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: true }, client, runtime });
    expect(runtime.signOutNative).toHaveBeenNthCalledWith(1, undefined);
    expect(deleteDesktopEnroll).not.toHaveBeenCalled();
  });

  it("completes when the server revocation fails", async () => {
    const runtime = fakeRuntime();
    vi.mocked(deleteDesktopEnroll).mockRejectedValue(apiError(401));
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    expect(runtime.signOutNative).toHaveBeenCalledTimes(2);
    expect(globalThis.sessionStorage.getItem(CACHE_KEY)).toBeNull();
  });

  it("retries the native teardown when the first pass fails", async () => {
    const runtime = fakeRuntime();
    vi.mocked(runtime.signOutNative).mockRejectedValueOnce(new Error("host busy")).mockResolvedValue(undefined);
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    expect(deleteDesktopEnroll).toHaveBeenCalledWith(client, "h");
    expect(runtime.signOutNative).toHaveBeenCalledTimes(2);
  });

  it("completes when the second native pass fails", async () => {
    const runtime = fakeRuntime();
    vi.mocked(runtime.signOutNative).mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("host busy"));
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    expect(globalThis.sessionStorage.getItem(CACHE_KEY)).toBeNull();
  });

  it("completes when the mode stash fails", async () => {
    const runtime = fakeRuntime();
    vi.mocked(runtime.saveApprovalModes).mockRejectedValue(new Error("host busy"));
    globalThis.localStorage.setItem("pidash:chat-approval-mode:r1", "ask");
    resume(runtime);
    await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    expect(runtime.signOutNative).toHaveBeenCalled();
  });

  it("survives an unwritable store", async () => {
    const runtime = fakeRuntime();
    resume(runtime);
    await connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    const removeItem = vi.fn(() => {
      throw new Error("denied");
    });
    vi.stubGlobal("sessionStorage", { getItem: () => null, setItem: () => undefined, removeItem });
    try {
      await disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    } finally {
      vi.unstubAllGlobals();
    }
    expect(removeItem).toHaveBeenCalled();
    expect(runtime.signOutNative).toHaveBeenCalled();
  });

  it("cleans up only after an in-flight enrollment settles", async () => {
    const runtime = fakeRuntime();
    let releaseProfile!: (value: AgentProfile) => void;
    const profileGate = new Promise<AgentProfile>((resolve) => {
      releaseProfile = resolve;
    });
    vi.mocked(getAgentProfile).mockReturnValue(profileGate);
    resume(runtime);
    const pending = connectProject({ workspaceSlug: "ws", projectId: "p", client, runtime });
    await waitFor(() => expect(getAgentProfile).toHaveBeenCalled());
    const disposal = disposeRuntimeForSignOut({ choice: { deleteChatHistory: false }, client, runtime });
    releaseProfile(profile());
    await pending;
    await disposal;
    const tokenOrder = vi.mocked(runtime.writeModelToken).mock.invocationCallOrder[0] ?? 0;
    const deleteOrder = vi.mocked(deleteDesktopEnroll).mock.invocationCallOrder[0] ?? 0;
    expect(tokenOrder).toBeGreaterThan(0);
    expect(deleteOrder).toBeGreaterThan(tokenOrder);
  });
});
