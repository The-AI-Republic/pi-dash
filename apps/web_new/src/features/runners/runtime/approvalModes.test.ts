// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentRuntimeApi } from "../../../core/platform/index.js";
import { APPROVAL_MODE_STORAGE_PREFIX, restoreApprovalModes, stashApprovalModes } from "./approvalModes.js";

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
  vi.stubGlobal("localStorage", memoryStorage());
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function fakeRuntime(): AgentRuntimeApi {
  return {
    discover: vi.fn(async () => ({ kind: "not_answering" as const, reason: "test" })),
    localRunners: vi.fn(async () => []),
    addEngineRunner: vi.fn(async () => ({
      runner: "r",
      runner_id: "id",
      workspace: "ws",
      project: "P",
      working_dir: "/t",
    })),
    installService: vi.fn(async () => undefined),
    ensureEngine: vi.fn(async () => undefined),
    engineLogin: vi.fn(async () => undefined),
    reloadCredentials: vi.fn(async () => undefined),
    requestReconnect: vi.fn(async () => undefined),
    saveApprovalModes: vi.fn(async () => undefined),
    loadApprovalModes: vi.fn(async () => ({})),
    scanLegacyTrees: vi.fn(async () => []),
    finishLegacyTree: vi.fn(async () => undefined),
    restoreLegacyFolder: vi.fn(async () => undefined),
    loadNotices: vi.fn(async () => ({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] })),
    recordNotices: vi.fn(async () => ({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] })),
    dismissNotices: vi.fn(async () => ({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] })),
    signOutComputer: vi.fn(async () => undefined),
  };
}

/** Minimal Storage double for edges the page store cannot produce. */
function fakeStorage(entries: Record<string, string>, keys: Array<string | null>): Storage {
  return {
    length: keys.length,
    key: (index: number) => keys[index] ?? null,
    getItem: (key: string) => entries[key] ?? null,
    setItem: vi.fn(),
    removeItem: vi.fn(),
    clear: vi.fn(),
  } as unknown as Storage;
}

describe("stashApprovalModes", () => {
  beforeEach(() => {
    globalThis.localStorage.clear();
  });

  it("does nothing without an account", async () => {
    const runtime = fakeRuntime();
    globalThis.localStorage.setItem(`${APPROVAL_MODE_STORAGE_PREFIX}r1`, "ask");
    await stashApprovalModes({ account: "", runtime });
    expect(runtime.saveApprovalModes).not.toHaveBeenCalled();
  });

  it("hands stored modes to the host and skips anything else", async () => {
    const runtime = fakeRuntime();
    globalThis.localStorage.setItem(`${APPROVAL_MODE_STORAGE_PREFIX}r1`, "ask");
    globalThis.localStorage.setItem(`${APPROVAL_MODE_STORAGE_PREFIX}r2`, "full_access");
    globalThis.localStorage.setItem(`${APPROVAL_MODE_STORAGE_PREFIX}bogus`, "sometimes");
    globalThis.localStorage.setItem("unrelated", "ask");
    await stashApprovalModes({ account: "u1", runtime });
    expect(runtime.saveApprovalModes).toHaveBeenCalledWith("u1", { r1: "ask", r2: "full_access" });
  });

  it("skips the host call when no modes are stored", async () => {
    const runtime = fakeRuntime();
    globalThis.localStorage.setItem("unrelated", "ask");
    await stashApprovalModes({ account: "u1", runtime });
    expect(runtime.saveApprovalModes).not.toHaveBeenCalled();
  });

  it("skips null keys and null values", async () => {
    const runtime = fakeRuntime();
    const storage = fakeStorage({}, [null]);
    await stashApprovalModes({ account: "u1", runtime, storage });
    expect(runtime.saveApprovalModes).not.toHaveBeenCalled();
    const nullValue = fakeStorage({}, [`${APPROVAL_MODE_STORAGE_PREFIX}r1`]);
    await stashApprovalModes({ account: "u1", runtime, storage: nullValue });
    expect(runtime.saveApprovalModes).not.toHaveBeenCalled();
  });

  it("does nothing without any storage", async () => {
    const runtime = fakeRuntime();
    vi.stubGlobal("localStorage", undefined);
    await stashApprovalModes({ account: "u1", runtime });
    expect(runtime.saveApprovalModes).not.toHaveBeenCalled();
  });
});

describe("restoreApprovalModes", () => {
  beforeEach(() => {
    globalThis.localStorage.clear();
  });

  it("does nothing without an account", async () => {
    const runtime = fakeRuntime();
    await restoreApprovalModes({ account: "", runtime });
    expect(runtime.loadApprovalModes).not.toHaveBeenCalled();
  });

  it("writes the host copy back and leaves newer local modes alone", async () => {
    const runtime = fakeRuntime();
    vi.mocked(runtime.loadApprovalModes).mockResolvedValue({ r1: "ask", r2: "workspace", r3: "bogus" });
    globalThis.localStorage.setItem(`${APPROVAL_MODE_STORAGE_PREFIX}r2`, "full_access");
    await restoreApprovalModes({ account: "u1", runtime });
    expect(runtime.loadApprovalModes).toHaveBeenCalledWith("u1");
    expect(globalThis.localStorage.getItem(`${APPROVAL_MODE_STORAGE_PREFIX}r1`)).toBe("ask");
    expect(globalThis.localStorage.getItem(`${APPROVAL_MODE_STORAGE_PREFIX}r2`)).toBe("full_access");
    expect(globalThis.localStorage.getItem(`${APPROVAL_MODE_STORAGE_PREFIX}r3`)).toBeNull();
  });

  it("does nothing without any storage", async () => {
    const runtime = fakeRuntime();
    vi.stubGlobal("localStorage", undefined);
    await restoreApprovalModes({ account: "u1", runtime });
    expect(runtime.loadApprovalModes).not.toHaveBeenCalled();
  });
});
