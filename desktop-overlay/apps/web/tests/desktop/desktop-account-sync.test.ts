/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * One sign-in for the app and this computer's `pidash` CLI: whichever signed
 * in or out most recently wins, checked on launch, focus and a timer.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DaemonAuthStatus } from "../../core/services/desktop-account-sync";

const invoke = vi.fn();
const request = vi.fn();

vi.mock("@pi-dash/constants", () => ({ API_BASE_URL: "https://pidash.test" }));
vi.mock("@/pi-dash-web/components/desktop/agent-runtime-edition", () => ({
  CSRF_TOKEN_PATH: "/api/auth/get-csrf-token/",
  AGENT_RUNTIME_REASON_MESSAGES: {},
}));
vi.mock("@pi-dash/services", () => ({
  APIService: class {
    request = request;
  },
}));

const EARLY = "2026-10-26T08:00:00Z";
const LATE = "2026-10-26T09:00:00Z";

function daemon(overrides: Partial<DaemonAuthStatus> = {}): DaemonAuthStatus {
  return {
    signed_in: true,
    cloud_url: "https://pidash.test",
    user: { id: "user-a", email: "a@example.test" },
    workspace_slug: "acme",
    dev_machine_id: "machine-1",
    changed_at: EARLY,
    daemon_running: true,
    active_runs: 0,
    ...overrides,
  };
}

const signedOut = (changed_at: string | null = EARLY) =>
  daemon({ signed_in: false, user: null, workspace_slug: null, changed_at });

let status: DaemonAuthStatus;
let desktopChangedAt: string | null;
let me: { status: number; id?: string };

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
  request.mockReset();
  sessionStorage.clear();
  status = daemon();
  desktopChangedAt = LATE;
  me = { status: 401 };
  (window as unknown as Record<string, unknown>).__TAURI__ = { core: { invoke } };
  invoke.mockImplementation(async (command: string, args?: { userId?: string | null }) => {
    switch (command) {
      case "pidash_auth_status":
        return status;
      case "desktop_auth_observe":
        return { user_id: args?.userId ?? null, changed_at: desktopChangedAt };
      case "daemon_ensure":
        return { mode: "service", cli_outdated: false, message: null };
      default:
        return undefined;
    }
  });
  request.mockImplementation(async ({ url }: { url: string }) => {
    if (url === "/api/auth/get-csrf-token/") return { data: { csrf_token: "csrf-1" } };
    if (url === "/api/v1/auth/device/start/") return { data: { device_code: "dev-1", user_code: "USER1" } };
    if (url === "/api/users/me/") {
      if (me.status === 200) return { data: { id: me.id } };
      throw Object.assign(new Error("request failed"), { response: { status: me.status } });
    }
    return { data: {} };
  });
});

const commands = () => invoke.mock.calls.map(([command]) => command as string);

describe("the sync rule", () => {
  it.each([
    ["app signed out, CLI signed out", null, null, signedOut(), "none"],
    ["app signed out, CLI signed in", null, null, daemon(), "handoff"],
    ["same account on both", "user-a", EARLY, daemon({ changed_at: LATE }), "none"],
    ["app newer, CLI signed out", "user-a", LATE, signedOut(EARLY), "sign_daemon_in"],
    ["app newer, CLI someone else", "user-b", LATE, daemon({ changed_at: EARLY }), "sign_daemon_in"],
    ["CLI newer and signed out", "user-a", EARLY, signedOut(LATE), "sign_desktop_out"],
    ["CLI newer as someone else", "user-b", EARLY, daemon({ changed_at: LATE }), "sign_desktop_out_then_handoff"],
    ["a tie goes to the app", "user-b", LATE, daemon({ changed_at: LATE }), "sign_daemon_in"],
    ["CLI never recorded a change", "user-a", EARLY, signedOut(null), "sign_daemon_in"],
    ["app never recorded a change", "user-a", null, signedOut(EARLY), "sign_desktop_out"],
    ["CLI signed in, account unknown", "user-a", LATE, daemon({ user: null, changed_at: EARLY }), "confirm_sign_daemon_in"],
    [
      "legacy CLI: account unknown, no change recorded",
      "user-a",
      LATE,
      daemon({ user: null, changed_at: null }),
      "confirm_sign_daemon_in",
    ],
    [
      "CLI flags its account as unknown",
      "user-a",
      LATE,
      daemon({ user_unknown: true, changed_at: null }),
      "confirm_sign_daemon_in",
    ],
    [
      "CLI newer, account unknown",
      "user-a",
      EARLY,
      daemon({ user: null, changed_at: LATE }),
      "sign_desktop_out_then_handoff",
    ],
  ] as const)("%s", async (_name, desktopUserId, changedAt, cli, expected) => {
    const { decideAccountSync } = await import("../../core/services/desktop-account-sync");
    expect(decideAccountSync({ desktopUserId, desktopChangedAt: changedAt, daemon: cli })).toBe(expected);
  });

  it("trusts an unknown CLI user the app's session was handed from", async () => {
    const { decideAccountSync } = await import("../../core/services/desktop-account-sync");
    const cli = daemon({ user: null, changed_at: null });
    expect(decideAccountSync({ desktopUserId: "user-a", desktopChangedAt: LATE, daemon: cli, handedOff: true })).toBe(
      "none"
    );
  });
});

describe("a sync pass", () => {
  const signOutDesktop = vi.fn(async () => {});

  beforeEach(() => signOutDesktop.mockClear());

  it("hands the CLI's sign-in to a signed-out app", async () => {
    const { syncAccounts } = await import("../../core/services/desktop-account-sync");
    await expect(syncAccounts({ signOutDesktop })).resolves.toBe("handoff");
    // The server said the app is signed out before anything was handed over.
    expect(request).toHaveBeenCalledWith(expect.objectContaining({ url: "/api/users/me/" }));
    expect(invoke).toHaveBeenCalledWith("desktop_auth_observe", { userId: null });
    expect(commands()).toContain("pidash_desktop_handoff");
    expect(signOutDesktop).not.toHaveBeenCalled();
  });

  it("does nothing when it cannot tell whether the app is signed in", async () => {
    me = { status: 503 };
    const { syncAccounts } = await import("../../core/services/desktop-account-sync");
    await expect(syncAccounts({ signOutDesktop })).resolves.toBe("none");
    expect(commands()).not.toContain("pidash_desktop_handoff");
    expect(commands()).not.toContain("desktop_auth_observe");
  });

  it("signs the CLI in as the app's user when the app's sign-in is newer", async () => {
    status = signedOut(EARLY);
    const { syncAccounts } = await import("../../core/services/desktop-account-sync");
    await expect(syncAccounts({ desktopUserId: "user-a", workspaceSlug: "acme", signOutDesktop })).resolves.toBe(
      "sign_daemon_in"
    );
    expect(request).toHaveBeenCalledWith(
      expect.objectContaining({ url: "/api/v1/auth/device/approve/", data: { user_code: "USER1" } })
    );
    expect(invoke).toHaveBeenCalledWith("pidash_cli_login", {
      deviceCode: "dev-1",
      cloudUrl: "https://pidash.test",
      workspace: "acme",
    });
    expect(commands().at(-1)).toBe("daemon_ensure");
  });

  describe("switching a CLI signed in as someone else", () => {
    const busy = () => daemon({ user: { id: "user-b", email: "b@example.test" }, changed_at: EARLY, active_runs: 2 });

    it("switches silently when nothing is running", async () => {
      status = daemon({ user: { id: "user-b", email: "b@example.test" }, changed_at: EARLY, active_runs: 0 });
      const sync = await import("../../core/services/desktop-account-sync");
      const prompt = vi.fn();
      sync.registerAccountSwitchPrompt(prompt);
      await expect(sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop })).resolves.toBe("sign_daemon_in");
      expect(prompt).not.toHaveBeenCalled();
      expect(commands()).toContain("pidash_cli_login");
    });

    it("asks first when runs are in progress, and switches on confirm", async () => {
      status = busy();
      const sync = await import("../../core/services/desktop-account-sync");
      const prompt = vi.fn((_request: unknown, resolve: (confirmed: boolean) => void) => resolve(true));
      sync.registerAccountSwitchPrompt(prompt);
      await expect(
        sync.syncAccounts({ desktopUserId: "user-a", desktopEmail: "a@example.test", signOutDesktop })
      ).resolves.toBe("sign_daemon_in");
      expect(prompt.mock.calls[0][0]).toEqual({
        daemonEmail: "b@example.test",
        desktopEmail: "a@example.test",
        activeRuns: 2,
      });
      expect(commands()).toContain("pidash_cli_login");
      expect(signOutDesktop).not.toHaveBeenCalled();
    });

    it("lets the CLI's account win when the switch is declined", async () => {
      status = busy();
      const sync = await import("../../core/services/desktop-account-sync");
      sync.registerAccountSwitchPrompt((_request, resolve) => resolve(false));
      await expect(sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop })).resolves.toBe(
        "sign_desktop_out_then_handoff"
      );
      expect(signOutDesktop).toHaveBeenCalledOnce();
      expect(commands()).not.toContain("pidash_cli_login");
    });

    it("asks before re-signing a CLI whose user is unknown, even with nothing running", async () => {
      status = daemon({ user: null, changed_at: null, active_runs: 0 });
      const sync = await import("../../core/services/desktop-account-sync");
      const prompt = vi.fn((_request: unknown, resolve: (confirmed: boolean) => void) => resolve(true));
      sync.registerAccountSwitchPrompt(prompt);
      await expect(
        sync.syncAccounts({ desktopUserId: "user-a", desktopEmail: "a@example.test", signOutDesktop })
      ).resolves.toBe("confirm_sign_daemon_in");
      expect(prompt.mock.calls[0][0]).toEqual({ daemonEmail: "", desktopEmail: "a@example.test", activeRuns: 0 });
      expect(commands()).toContain("pidash_cli_login");
    });

    it("does not ask again about the same state once declined", async () => {
      status = daemon({ user: null, changed_at: null, active_runs: 3 });
      const sync = await import("../../core/services/desktop-account-sync");
      const prompt = vi.fn((_request: unknown, resolve: (confirmed: boolean) => void) => resolve(false));
      sync.registerAccountSwitchPrompt(prompt);
      await expect(sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop })).resolves.toBe(
        "sign_desktop_out_then_handoff"
      );
      // The 15 s timer and focus keep calling; the answer stands.
      await sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop });
      await sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop });
      expect(prompt).toHaveBeenCalledOnce();
      expect(signOutDesktop).toHaveBeenCalledOnce();
      expect(commands()).not.toContain("pidash_cli_login");
    });

    it("after a decline hands the CLI's account over and then leaves it be", async () => {
      // Declined: the app was signed out (which wipes webview storage) and
      // the next pass hands the unknown CLI user's account to the app.
      status = daemon({ user: null, changed_at: null, active_runs: 3 });
      const sync = await import("../../core/services/desktop-account-sync");
      const prompt = vi.fn((_request: unknown, resolve: (confirmed: boolean) => void) => resolve(false));
      sync.registerAccountSwitchPrompt(prompt);
      await expect(sync.syncAccounts({ signOutDesktop })).resolves.toBe("handoff");
      // Signed in by the handoff, with a newer app-side change than the CLI's.
      await expect(sync.syncAccounts({ desktopUserId: "user-z", signOutDesktop })).resolves.toBe("none");
      expect(prompt).not.toHaveBeenCalled();
      expect(signOutDesktop).not.toHaveBeenCalled();
    });

    it("forgets a handoff that failed, so a later sign-in is still asked about", async () => {
      status = daemon({ user: null, changed_at: null, active_runs: 0 });
      const sync = await import("../../core/services/desktop-account-sync");
      const base = invoke.getMockImplementation()!;
      invoke.mockImplementation(async (command: string, args?: { userId?: string | null }) => {
        if (command === "pidash_desktop_handoff") throw { code: "no_handoff_key", message: "no key" };
        return base(command, args);
      });
      const prompt = vi.fn((_request: unknown, resolve: (confirmed: boolean) => void) => resolve(true));
      sync.registerAccountSwitchPrompt(prompt);
      await expect(sync.syncAccounts({ signOutDesktop })).rejects.toBeDefined();
      await expect(sync.syncAccounts({ desktopUserId: "user-z", signOutDesktop })).resolves.toBe(
        "confirm_sign_daemon_in"
      );
      expect(prompt).toHaveBeenCalledOnce();
    });

    it("never stops runs without asking, even with no dialog mounted", async () => {
      status = busy();
      const sync = await import("../../core/services/desktop-account-sync");
      sync.registerAccountSwitchPrompt(null);
      await sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop });
      expect(commands()).not.toContain("pidash_cli_login");
      expect(signOutDesktop).toHaveBeenCalledOnce();
    });
  });

  it("signs the app out when the CLI's change is newer", async () => {
    status = daemon({ user: { id: "user-b", email: "b@example.test" }, changed_at: LATE });
    desktopChangedAt = EARLY;
    const { syncAccounts } = await import("../../core/services/desktop-account-sync");
    await expect(syncAccounts({ desktopUserId: "user-a", signOutDesktop })).resolves.toBe(
      "sign_desktop_out_then_handoff"
    );
    expect(signOutDesktop).toHaveBeenCalledOnce();
    expect(commands()).not.toContain("pidash_cli_login");
    expect(commands()).not.toContain("pidash_auth_logout");
  });

  it("does not retry a failed action every pass, but does once the state changes", async () => {
    const sync = await import("../../core/services/desktop-account-sync");
    const handoffs = () => commands().filter((command) => command === "pidash_desktop_handoff").length;
    invoke.mockImplementation(async (command: string, args?: { userId?: string | null }) => {
      if (command === "pidash_auth_status") return status;
      if (command === "desktop_auth_observe") return { user_id: args?.userId ?? null, changed_at: desktopChangedAt };
      if (command === "pidash_desktop_handoff") throw { code: "handoff_failed", message: "old server" };
      return undefined;
    });
    await expect(sync.syncAccounts({ signOutDesktop })).rejects.toEqual(expect.objectContaining({ code: "handoff_failed" }));
    await expect(sync.syncAccounts({ signOutDesktop })).resolves.toBe("none");
    expect(handoffs()).toBe(1);
    status = daemon({ changed_at: LATE });
    await expect(sync.syncAccounts({ signOutDesktop })).rejects.toBeDefined();
    expect(handoffs()).toBe(2);
  });

  it("leaves an outdated CLI alone and says it needs an update", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "pidash_auth_status") throw { code: "cli_outdated", message: "too old" };
      return undefined;
    });
    const sync = await import("../../core/services/desktop-account-sync");
    await expect(sync.syncAccounts({ desktopUserId: "user-a", signOutDesktop })).resolves.toBe("none");
    expect(sync.getDaemonNotice().cliOutdated).toBe(true);
    expect(commands()).toEqual(["pidash_auth_status"]);
  });

  it("shares one pass between overlapping triggers", async () => {
    status = daemon();
    const { syncAccounts } = await import("../../core/services/desktop-account-sync");
    const passes = await Promise.all([
      syncAccounts({ desktopUserId: "user-a", signOutDesktop }),
      syncAccounts({ desktopUserId: "user-a", signOutDesktop }),
    ]);
    expect(passes).toEqual(["none", "none"]);
    expect(commands().filter((command) => command === "pidash_auth_status")).toHaveLength(1);
  });
});

describe("the daemon notice", () => {
  it("tracks the daemon's mode and offers repair and update", async () => {
    const sync = await import("../../core/services/desktop-account-sync");
    const seen: string[] = [];
    const stop = sync.subscribeDaemonNotice(() => seen.push(sync.getDaemonNotice().daemon?.mode ?? "none"));
    invoke.mockResolvedValueOnce({ mode: "child", cli_outdated: false, message: "launchd refused" });
    await sync.ensureSharedDaemon();
    invoke.mockResolvedValueOnce({ mode: "service", cli_outdated: false, message: null });
    await sync.repairSharedDaemon();
    expect(commands()).toEqual(["daemon_ensure", "daemon_repair"]);
    expect(seen.filter((mode, i) => mode !== seen[i - 1])).toEqual(["child", "service"]);
    invoke.mockClear();
    await sync.updateCli();
    expect(commands()).toEqual(["install_pidash_cli", "daemon_ensure"]);
    stop();
  });
});
