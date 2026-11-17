// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The reconciliation supervisor (desktop_unified_signin §6.2): each check
// performs exactly the action the pure reconcile decided — handoff, CLI
// sign-in, app sign-out, CLI logout — and advances `last_seen_seq` only
// once the action landed.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  client: { baseUrl: "https://cloud.test" },
  agentRuntime: undefined as
    | {
        daemonHello: ReturnType<typeof vi.fn>;
        daemonActiveRuns: ReturnType<typeof vi.fn>;
        desktopSessionHandoff: ReturnType<typeof vi.fn>;
        getAuthSyncRecord: ReturnType<typeof vi.fn>;
        setAuthSyncRecord: ReturnType<typeof vi.fn>;
        markFreshSignIn: ReturnType<typeof vi.fn>;
        signOutComputer: ReturnType<typeof vi.fn>;
      }
    | undefined,
  me: null as { id?: string | undefined; email?: string | undefined; cloudUrl?: string } | null,
  resumeRuntime: vi.fn((_input: unknown) => undefined),
  signInDaemonForAccount: vi.fn((_input: unknown) => Promise.resolve()),
  performSignOut: vi.fn((_input: unknown) => Promise.resolve("/")),
  notifyInfo: vi.fn(),
  navigate: vi.fn(),
}));

vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => harness.client,
}));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return {
      kind: "desktop",
      apiOrigin: "https://cloud.test",
      agentRuntime: harness.agentRuntime,
      cliSetup: {},
      onFocusChange: () => () => undefined,
    };
  },
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useMe: () => ({ data: harness.me }) };
});

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useRouter: () => ({ navigate: harness.navigate }) };
});

vi.mock("./service.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./service.js")>();
  return {
    ...original,
    resumeRuntime: (input: unknown) => harness.resumeRuntime(input),
    signInDaemonForAccount: (input: unknown) => harness.signInDaemonForAccount(input),
  };
});

vi.mock("../../auth/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../auth/index.js")>();
  return { ...original, performSignOut: (input: unknown) => harness.performSignOut(input) };
});

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyInfo: (...args: Array<unknown>) => harness.notifyInfo(...args) };
});

import { AuthSyncSupervisor } from "./SyncSupervisor.js";

const CLOUD = "https://cloud.test";

function runtime(hello: unknown, record: unknown = null) {
  harness.agentRuntime = {
    daemonHello: vi.fn(async () => hello),
    daemonActiveRuns: vi.fn(async () => 0),
    desktopSessionHandoff: vi.fn(async () => ({ user_id: "u1" })),
    getAuthSyncRecord: vi.fn(async () => record),
    setAuthSyncRecord: vi.fn(async () => undefined),
    markFreshSignIn: vi.fn(async () => undefined),
    signOutComputer: vi.fn(async () => undefined),
  };
  return harness.agentRuntime;
}

function renderSupervisor() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <AuthSyncSupervisor />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  harness.me = null;
  harness.agentRuntime = undefined;
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("AuthSyncSupervisor", () => {
  it("is inert without the desktop capability", () => {
    harness.agentRuntime = undefined;
    renderSupervisor();
    expect(screen.queryByText("Sign in and switch")).toBeNull();
  });

  it("signs in through the handoff when the daemon signed in first (no user yet)", async () => {
    const rt = runtime({
      daemon_version: "1.0.0",
      ipc_version: 7,
      cloud_url: CLOUD,
      account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
      auth_state: "signed_in",
      auth_event: {
        seq: 1,
        kind: "signed_in",
        at: "2026-11-16T10:00:00Z",
        account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
      },
      workspaces: [],
      engine: "available",
    });
    renderSupervisor();
    await vi.waitFor(() => expect(rt.desktopSessionHandoff).toHaveBeenCalledWith("u1"));
    expect(harness.navigate).toHaveBeenCalledWith({ to: "/" });
    expect(rt.setAuthSyncRecord).toHaveBeenCalledWith(
      expect.objectContaining({ last_seen_seq: 1, fresh_sign_in_at: null })
    );
  });

  it("falls back to the browser sign-in (once per account) when the handoff fails", async () => {
    const rt = runtime({
      daemon_version: "1.0.0",
      ipc_version: 7,
      cloud_url: CLOUD,
      account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
      auth_state: "signed_in",
      auth_event: {
        seq: 1,
        kind: "signed_in",
        at: "2026-11-16T10:00:00Z",
        account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
      },
      workspaces: [],
      engine: "available",
    });
    rt.desktopSessionHandoff.mockRejectedValueOnce(new Error("cannot hand off"));
    renderSupervisor();
    await vi.waitFor(() => expect(harness.notifyInfo).toHaveBeenCalledTimes(1));
    expect(rt.desktopSessionHandoff).toHaveBeenCalledWith("u1");
    expect(harness.navigate).not.toHaveBeenCalled();
  });

  it("signs the app out with its notice when the CLI signed out newest", async () => {
    harness.me = { id: "u1", email: "u1@test", cloudUrl: CLOUD };
    const rt = runtime(
      {
        daemon_version: "1.0.0",
        ipc_version: 7,
        cloud_url: CLOUD,
        auth_state: "signed_out",
        auth_event: {
          seq: 4,
          kind: "signed_out",
          reason: "logout",
          at: "2026-11-16T10:00:00Z",
          account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
        },
        workspaces: [],
        engine: "available",
      },
      {
        own_event: {
          kind: "signed_in",
          account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
          at: "2026-11-16T09:00:00Z",
        },
        last_seen_seq: 2,
      }
    );
    renderSupervisor();
    await vi.waitFor(() =>
      expect(harness.notifyInfo).toHaveBeenCalledWith(
        "Signed out from this computer",
        "You were signed out because Pi Dash on this computer was signed out."
      )
    );
    expect(harness.performSignOut).toHaveBeenCalledWith(expect.objectContaining({ skipRuntimeConfirm: true }));
    expect(rt.setAuthSyncRecord).toHaveBeenCalledWith(expect.objectContaining({ last_seen_seq: 4 }));
  });

  it("signs the CLI in when the app signed in newest", async () => {
    harness.me = { id: "u1", email: "u1@test", cloudUrl: CLOUD };
    runtime(
      {
        daemon_version: "1.0.0",
        ipc_version: 7,
        cloud_url: CLOUD,
        auth_state: "signed_out",
        auth_event: { seq: 2, kind: "signed_out", reason: "logout", at: "2026-11-16T09:00:00Z" },
        workspaces: [],
        engine: "available",
      },
      {
        own_event: {
          kind: "signed_in",
          account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
          at: "2026-11-16T10:00:00Z",
        },
        last_seen_seq: 2,
      }
    );
    renderSupervisor();
    await vi.waitFor(() => expect(harness.signInDaemonForAccount).toHaveBeenCalledTimes(1));
  });

  it("stamps the app's own sign-in from a fresh marker, and clips it on restore", async () => {
    harness.me = { id: "u1", email: "u1@test", cloudUrl: CLOUD };
    const fresh = new Date().toISOString();
    const rt = runtime(
      {
        daemon_version: "1.0.0",
        ipc_version: 7,
        cloud_url: CLOUD,
        auth_state: "signed_in",
        workspaces: [],
        engine: "available",
        account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
      },
      { own_event: null, last_seen_seq: null, fresh_sign_in_at: fresh }
    );
    renderSupervisor();
    await vi.waitFor(() =>
      expect(
        rt.setAuthSyncRecord.mock.calls.some((call) => (call[0] as Record<string, unknown>).fresh_sign_in_at === null)
      ).toBe(true)
    );
    const wrote = rt.setAuthSyncRecord.mock.calls.map((call) => call[0] as Record<string, unknown>);
    expect(wrote.some((r) => (r.own_event as { kind?: string } | null)?.kind === "signed_in")).toBe(true);
  });

  it("asks before a switch with runs in flight, and switches only on confirm", async () => {
    const user = userEvent.setup();
    harness.me = { id: "u2", email: "u2@test", cloudUrl: CLOUD };
    const rt = runtime(
      {
        daemon_version: "1.0.0",
        ipc_version: 7,
        cloud_url: CLOUD,
        account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
        auth_state: "signed_in",
        auth_event: {
          seq: 3,
          kind: "signed_in",
          at: "2026-11-16T08:00:00Z",
          account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
        },
        workspaces: [],
        engine: "available",
      },
      {
        own_event: {
          kind: "signed_in",
          account: { user_id: "u2", email: "u2@test", cloud_url: CLOUD },
          at: "2026-11-16T10:00:00Z",
        },
        last_seen_seq: 3,
      }
    );
    rt.daemonActiveRuns.mockResolvedValueOnce(2);
    renderSupervisor();
    expect(await screen.findByText(/Switch this computer's sign-in\?/)).toBeVisible();
    expect(harness.signInDaemonForAccount).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Sign in and switch" }));
    await vi.waitFor(() => expect(harness.signInDaemonForAccount).toHaveBeenCalledTimes(1));
  });

  it("does not re-ask after a cancel while the risk is unchanged", async () => {
    const user = userEvent.setup();
    harness.me = { id: "u2", email: "u2@test", cloudUrl: CLOUD };
    const rt = runtime(
      {
        daemon_version: "1.0.0",
        ipc_version: 7,
        cloud_url: CLOUD,
        account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
        auth_state: "signed_in",
        auth_event: {
          seq: 3,
          kind: "signed_in",
          at: "2026-11-16T08:00:00Z",
          account: { user_id: "u1", email: "u1@test", cloud_url: CLOUD },
        },
        workspaces: [],
        engine: "available",
      },
      {
        own_event: {
          kind: "signed_in",
          account: { user_id: "u2", email: "u2@test", cloud_url: CLOUD },
          at: "2026-11-16T10:00:00Z",
        },
        last_seen_seq: 3,
      }
    );
    rt.daemonActiveRuns.mockResolvedValue(2);
    renderSupervisor();
    expect(await screen.findByText(/Switch this computer's sign-in\?/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Keep the app only" }));
    expect(screen.queryByText(/Switch this computer's sign-in\?/)).toBeNull();
    expect(harness.signInDaemonForAccount).not.toHaveBeenCalled();
    expect(rt.setAuthSyncRecord).toHaveBeenCalledWith(expect.objectContaining({ last_seen_seq: 3 }));
  });
});
