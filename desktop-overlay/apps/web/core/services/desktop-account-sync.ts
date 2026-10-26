/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * One `pidash` daemon and one sign-in for this computer.
 *
 * The app no longer runs private per-workspace daemons. It uses the user's
 * `pidash` CLI at its default paths: the host makes sure the CLI is installed
 * and its daemon runs (as the OS service, or as a child of the app when the
 * service cannot be installed), and this module keeps the app's sign-in and
 * the CLI's in step.
 *
 * Account sync — "most recent sign-in or sign-out wins" — runs on launch, on
 * window focus and every 15 s:
 *
 * - app signed out, CLI signed in → hand the CLI's sign-in to the app (the
 *   host loads a one-time redeem link in the window);
 * - app signed in, CLI signed out or someone else → whichever changed last
 *   wins: the app's newer → sign the CLI in as the app's user (device-code
 *   grant the app approves itself); the CLI's newer → sign the app out, after
 *   which the next pass hands the CLI's account over if it has one.
 *
 * Re-signing a CLI that may belong to someone else is never silent when it
 * could stop runs or when the CLI's user is unknown (a config from before the
 * CLI recorded it): the user is asked, and declining lets the CLI's account
 * win.
 *
 * The app's own last-change time is kept by the host (`desktop_auth_observe`),
 * outside the webview storage that sign-out wipes.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@pi-dash/services";
import { CSRF_TOKEN_PATH } from "@/pi-dash-web/components/desktop/agent-runtime-edition";

class SyncAPI extends APIService {}
const syncAPI = new SyncAPI(API_BASE_URL);

type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

/** `pidash auth status --json`. */
export type DaemonAuthStatus = {
  signed_in: boolean;
  cloud_url: string | null;
  user: { id: string; email: string } | null;
  workspace_slug: string | null;
  dev_machine_id: string | null;
  changed_at: string | null;
  daemon_running: boolean;
  active_runs: number;
  /**
   * The CLI is signed in but cannot say as whom (a config from before it
   * recorded the user). Older CLIs omit it; `signed_in` with no `user` means
   * the same.
   */
  user_unknown?: boolean;
};

/** Signed in, but as nobody the CLI can name. */
export function daemonUserUnknown(daemon: DaemonAuthStatus): boolean {
  return daemon.signed_in && (daemon.user_unknown === true || !daemon.user?.id);
}

/** Who runs the shared daemon, as the host reports it. */
export type DaemonMode = "service" | "child" | "idle" | "unavailable";
/** A runner still in a legacy per-workspace config: adopt could not move it. */
export type UnadoptedRunner = { name: string; workspace: string; reason: string };
export type DaemonStatus = {
  mode: DaemonMode;
  cli_outdated: boolean;
  message: string | null;
  /** Older hosts omit it. */
  unadopted?: UnadoptedRunner[];
};

/** The app's signed-in user and when that last changed, as the host keeps it. */
export type DesktopAuthMark = { user_id: string | null; changed_at: string | null };

export type SyncDecision =
  | "none"
  | "handoff"
  | "sign_daemon_in"
  /** The app wins, but the CLI's user is unknown: ask before re-signing it. */
  | "confirm_sign_daemon_in"
  | "sign_desktop_out"
  | "sign_desktop_out_then_handoff";

export const CLI_OUTDATED_MESSAGE = "The pidash CLI on this computer is out of date. Update the Pi Dash CLI.";

function isDesktopHost() {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

function invoke<T>(command: string, args?: Record<string, unknown>) {
  return (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<T>(command, args);
}

/** The code of a host `CliError` rejection (`cli_outdated`, `no_handoff_key`, …). */
export function cliErrorCode(error: unknown): string | undefined {
  const code = (error as { code?: unknown } | null)?.code;
  return typeof code === "string" ? code : undefined;
}

export function cliErrorMessage(error: unknown): string {
  if (cliErrorCode(error) === "cli_outdated") return CLI_OUTDATED_MESSAGE;
  const message = (error as { message?: unknown } | null)?.message;
  if (typeof message === "string" && message) return message;
  return typeof error === "string" ? error : "The pidash CLI failed.";
}

// ---------------------------------------------------------------------------
// Daemon notice: what the corner strip shows (Repair / Update the CLI)
// ---------------------------------------------------------------------------

export type DaemonNotice = { daemon: DaemonStatus | null; cliOutdated: boolean; busy: boolean };

let notice: DaemonNotice = { daemon: null, cliOutdated: false, busy: false };
const listeners = new Set<() => void>();

function setNotice(next: Partial<DaemonNotice>) {
  notice = { ...notice, ...next };
  for (const listener of listeners) listener();
}

export function getDaemonNotice(): DaemonNotice {
  return notice;
}

export function subscribeDaemonNotice(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Record a daemon status the host returned, from whichever command. */
export function reportDaemonStatus(status: DaemonStatus) {
  setNotice({ daemon: status, cliOutdated: status.cli_outdated });
}

async function withBusy<T>(action: () => Promise<T>): Promise<T> {
  setNotice({ busy: true });
  try {
    return await action();
  } finally {
    setNotice({ busy: false });
  }
}

/** Install the CLI if needed, adopt legacy configs, and start the daemon. */
export async function ensureSharedDaemon(): Promise<DaemonStatus | null> {
  if (!isDesktopHost()) return null;
  const status = await invoke<DaemonStatus>("daemon_ensure");
  reportDaemonStatus(status);
  return status;
}

/** The Repair action: retry installing the daemon as an OS service. */
export async function repairSharedDaemon(): Promise<DaemonStatus> {
  return withBusy(async () => {
    const status = await invoke<DaemonStatus>("daemon_repair");
    reportDaemonStatus(status);
    return status;
  });
}

/** Replace an outdated CLI with the one this app ships, then start over. */
export async function updateCli(): Promise<DaemonStatus | null> {
  return withBusy(async () => {
    await invoke<void>("install_pidash_cli");
    return ensureSharedDaemon();
  });
}

// ---------------------------------------------------------------------------
// The CLI's sign-in
// ---------------------------------------------------------------------------

export async function getDaemonAuthStatus(): Promise<DaemonAuthStatus> {
  try {
    const status = await invoke<DaemonAuthStatus>("pidash_auth_status");
    if (notice.cliOutdated && !notice.daemon?.cli_outdated) setNotice({ cliOutdated: false });
    return status;
  } catch (error) {
    if (cliErrorCode(error) === "cli_outdated") setNotice({ cliOutdated: true });
    throw error;
  }
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const csrf = await syncAPI.request({ url: CSRF_TOKEN_PATH, method: "GET" });
  const response = await syncAPI.request({
    url: path,
    method: "POST",
    headers: { "Content-Type": "application/json", "X-CSRFTOKEN": (csrf.data as { csrf_token: string }).csrf_token },
    data: body,
  });
  return response.data as T;
}

/**
 * Sign the CLI (and so the shared daemon) in as the app's user: both halves
 * of the web `/auth/device/` page run by the app, then the CLI finishes the
 * exchange at its default paths and owns everything it writes.
 */
export async function signDaemonIn(workspaceSlug?: string): Promise<void> {
  const grant = await post<{ device_code: string; user_code: string }>("/api/v1/auth/device/start/", {});
  await post("/api/v1/auth/device/approve/", { user_code: grant.user_code });
  await invoke<void>("pidash_cli_login", {
    deviceCode: grant.device_code,
    cloudUrl: API_BASE_URL,
    workspace: workspaceSlug,
  });
  // The daemon refuses to start signed out; now it can.
  await ensureSharedDaemon();
}

/**
 * `pidash auth logout --yes` — the caller has already warned about runs in
 * progress. No CLI at all means nothing to sign out.
 */
export async function signDaemonOut(): Promise<void> {
  try {
    await invoke<void>("pidash_auth_logout");
  } catch (error) {
    if (cliErrorCode(error) === "cli_missing") return;
    throw new Error(`Could not sign out the Pi Dash CLI on this computer: ${cliErrorMessage(error)}`, {
      cause: error,
    });
  }
}

/** Runs the daemon has in flight; 0 when it cannot say. */
export async function activeDaemonRuns(): Promise<number> {
  if (!isDesktopHost()) return 0;
  try {
    const status = await getDaemonAuthStatus();
    return status.signed_in ? status.active_runs : 0;
  } catch {
    return 0;
  }
}

/** Tell the host who the app is signed in as (`null`: signed out). */
export async function observeDesktopAuth(userId: string | null): Promise<DesktopAuthMark> {
  return invoke<DesktopAuthMark>("desktop_auth_observe", { userId });
}

// ---------------------------------------------------------------------------
// Account sync
// ---------------------------------------------------------------------------

/** The rule, on its own so it can be tested without a host. */
export function decideAccountSync(input: {
  desktopUserId: string | null;
  desktopChangedAt: string | null;
  daemon: DaemonAuthStatus;
  /**
   * The app's session came from this very CLI sign-in by a handoff, so they
   * are the same user even when the CLI cannot say who that is.
   */
  handedOff?: boolean;
}): SyncDecision {
  const { desktopUserId, desktopChangedAt, daemon } = input;
  if (!desktopUserId) return daemon.signed_in ? "handoff" : "none";
  const userUnknown = daemonUserUnknown(daemon);
  if (daemon.signed_in && !userUnknown && daemon.user?.id === desktopUserId) return "none";
  if (userUnknown && input.handedOff) return "none";
  const desktopAt = Date.parse(desktopChangedAt ?? "");
  const daemonAt = Date.parse(daemon.changed_at ?? "");
  // A tie, or a CLI that never recorded a change, goes to the app: it is the
  // side the user is looking at. But a CLI signed in as someone it cannot
  // name may be another person's — that is the user's call, not ours.
  const daemonNewer = Number.isFinite(daemonAt) && (!Number.isFinite(desktopAt) || daemonAt > desktopAt);
  if (!daemonNewer) return userUnknown ? "confirm_sign_daemon_in" : "sign_daemon_in";
  return daemon.signed_in ? "sign_desktop_out_then_handoff" : "sign_desktop_out";
}

/**
 * The app's signed-in user, asked of the server when the caller does not
 * know it. `undefined` when the answer is unknown (offline), which skips the
 * pass rather than mistaking an outage for a sign-out.
 */
async function resolveDesktopUser(known: string | null | undefined): Promise<string | null | undefined> {
  if (known) return known;
  try {
    const response = await syncAPI.request({ url: "/api/users/me/", method: "GET" });
    return (response.data as { id?: string } | undefined)?.id || null;
  } catch (error) {
    const status = (error as { response?: { status?: number } }).response?.status;
    return status === 401 || status === 403 ? null : undefined;
  }
}

/** What the account-switch confirmation shows. `daemonEmail` is empty when the CLI cannot name its user. */
export type AccountSwitch = { daemonEmail: string; desktopEmail: string; activeRuns: number };
type AccountSwitchPrompt = (request: AccountSwitch, resolve: (confirmed: boolean) => void) => void;

let switchPrompt: AccountSwitchPrompt | null = null;

/** Mounts (or, with `null`, unmounts) the dialog {@link confirmAccountSwitch} opens. */
export function registerAccountSwitchPrompt(prompt: AccountSwitchPrompt | null) {
  switchPrompt = prompt;
}

/**
 * Ask before switching a CLI that has runs in progress to the app's account.
 * With no dialog to ask through, the answer is no: the switch would stop
 * runs without the user having agreed.
 */
export function confirmAccountSwitch(request: AccountSwitch): Promise<boolean> {
  const prompt = switchPrompt;
  if (!prompt) return Promise.resolve(false);
  return new Promise((resolve) => prompt(request, resolve));
}

// The last action attempted for a given state, so a failure (an older server
// without the handoff endpoint, a CLI bound to another cloud) is not retried
// every 15 s. Session storage survives the handoff's full-page navigations.
const ATTEMPT_KEY = "pidash-account-sync-attempt";

function lastAttempt(): string | null {
  try {
    return sessionStorage.getItem(ATTEMPT_KEY);
  } catch {
    return null;
  }
}

function rememberAttempt(key: string) {
  try {
    sessionStorage.setItem(ATTEMPT_KEY, key);
  } catch {
    // Without storage a failure is retried on the next pass; still correct.
  }
}

// The CLI sign-in the app's session was last handed from. It lets a pass
// recognise "same user" when the CLI cannot name its user — otherwise
// declining a switch (which hands the CLI's account to the app) would only
// lead to the same question again. Signing the app out wipes it with the rest
// of the webview's storage.
const HANDOFF_KEY = "pidash-account-sync-handoff";

function daemonSignIn(daemon: DaemonAuthStatus): string {
  return [daemon.cloud_url ?? "", daemon.dev_machine_id ?? "", daemon.workspace_slug ?? "", daemon.changed_at ?? ""].join(
    "|"
  );
}

function handedOffFrom(): string | null {
  try {
    return sessionStorage.getItem(HANDOFF_KEY);
  } catch {
    return null;
  }
}

function rememberHandoff(daemon: DaemonAuthStatus) {
  try {
    sessionStorage.setItem(HANDOFF_KEY, daemonSignIn(daemon));
  } catch {
    // Without storage an unknown CLI user is asked about again; still safe.
  }
}

function forgetHandoff() {
  try {
    sessionStorage.removeItem(HANDOFF_KEY);
  } catch {
    // Nothing was stored.
  }
}

export type SyncOptions = {
  /** The app's user as the store knows it; omitted or empty asks the server. */
  desktopUserId?: string | null;
  /** The app user's email, for the switch confirmation. */
  desktopEmail?: string;
  /** Where the user is, for binding the CLI when signing it in. */
  workspaceSlug?: string;
  /** Sign the app out *without* signing the CLI out (it is the CLI being followed). */
  signOutDesktop: () => Promise<void>;
};

let inFlight: Promise<SyncDecision> | null = null;

/** One account-sync pass. Concurrent calls share the pass in flight. */
export function syncAccounts(options: SyncOptions): Promise<SyncDecision> {
  if (!isDesktopHost()) return Promise.resolve("none");
  inFlight ??= runSync(options).finally(() => {
    inFlight = null;
  });
  return inFlight;
}

async function runSync(options: SyncOptions): Promise<SyncDecision> {
  let daemon: DaemonAuthStatus;
  try {
    daemon = await getDaemonAuthStatus();
  } catch {
    // No CLI, or one too old to say: nothing to sync with (the notice says so).
    return "none";
  }
  const desktopUserId = await resolveDesktopUser(options.desktopUserId);
  if (desktopUserId === undefined) return "none";
  const mark = await observeDesktopAuth(desktopUserId);
  const decision = decideAccountSync({
    desktopUserId,
    desktopChangedAt: mark.changed_at,
    daemon,
    handedOff: handedOffFrom() === daemonSignIn(daemon),
  });
  if (decision === "none") return decision;

  const key = [decision, desktopUserId ?? "", daemon.user?.id ?? "", daemon.changed_at ?? "", mark.changed_at ?? ""].join(
    "|"
  );
  if (lastAttempt() === key) return "none";
  rememberAttempt(key);

  switch (decision) {
    case "handoff":
      // The host loads the redeem link in this window; the page goes away.
      rememberHandoff(daemon);
      try {
        await invoke<void>("pidash_desktop_handoff");
      } catch (error) {
        forgetHandoff();
        throw error;
      }
      break;
    case "sign_daemon_in":
    case "confirm_sign_daemon_in":
      // Switching a CLI that is signed in as someone else stops its runs, and
      // one whose user is unknown may not be the app's user at all, so
      // neither is silent. Declining means the CLI's account wins: sign the
      // app out, and the next pass hands the CLI's account over — the two
      // never stay mismatched. The attempt is remembered above, so polling
      // does not ask again about the same state.
      if (decision === "confirm_sign_daemon_in" || (daemon.signed_in && daemon.active_runs > 0)) {
        const confirmed = await confirmAccountSwitch({
          daemonEmail: daemon.user?.email ?? "",
          desktopEmail: options.desktopEmail ?? "",
          activeRuns: daemon.active_runs,
        });
        if (!confirmed) {
          await options.signOutDesktop();
          return "sign_desktop_out_then_handoff";
        }
      }
      await signDaemonIn(options.workspaceSlug);
      break;
    case "sign_desktop_out":
    case "sign_desktop_out_then_handoff":
      // The sign-out reloads the app signed out; the pass on that launch
      // finds "app signed out, CLI signed in" and hands the CLI's account over.
      await options.signOutDesktop();
      break;
  }
  return decision;
}
