/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The app as a client of this computer's Pi Dash daemon
 * (`.ai_design/built_engine_as_runner/design.md` §4.1, §4.4, §4.6).
 *
 * The daemon is the `pidash` CLI's, running as a user service. The app finds
 * it at the standard control socket (`daemon.rs`), never starts or stops it,
 * and talks to it for local chat, engine settings and runner set-up. This
 * module holds what the app last learned about it (`Hello`, classified by the
 * host into a {@link Discovery}) and the sign-in rules that follow from it:
 *
 * - the engine credential is renewed **silently** only when the daemon reports
 *   the engine sign-in state `expiring` or `expired`;
 * - `never` gets a one-click "Turn on the built-in engine", `signed_out` and
 *   `revoked` only an explicit "Sign in the built-in engine" — the user signed
 *   it out, and the server refuses a session approval for it anyway.
 *
 * Server calls (the device grants) use the app's session; the daemon's own
 * credentials never pass through the webview.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@pi-dash/services";
import { CSRF_TOKEN_PATH } from "@/pi-dash-web/components/desktop/agent-runtime-edition";

// --- Host record shapes (serde snake_case, `daemon.rs` / pidash-ipc) -------

export type EngineState = "available" | "signed_out" | "expiring" | "unavailable";
export type EngineSignInState = "never" | "active" | "expiring" | "expired" | "signed_out" | "revoked";

export interface EngineStatusInfo {
  state: EngineState;
  reason_code?: string;
  sign_in_state: EngineSignInState;
  credential_expires_at?: string;
  model?: string;
  binary?: string;
  profile_fetched_at?: string;
  /** The owner's "Pause the built-in engine" switch (design §8). */
  paused_by_owner?: boolean;
  /** The server's automatic pause reason, when paused automatically. */
  auto_pause_reason?: string;
  /** When the daemon last read the pause state; absent = unknown. */
  pause_checked_at?: string;
}

/**
 * The pause control to show — one, never both: `resume` when the owner paused
 * the engine, `pause` when they did not, `none` while the daemon has not read
 * the pause state yet. `autoReason` is set when the server paused it on its
 * own (which the owner's switch does not lift).
 */
export function pauseControl(detail: EngineStatusInfo | undefined): {
  action: "pause" | "resume" | "none";
  autoReason?: string;
} {
  if (!detail?.pause_checked_at) return { action: "none" };
  return {
    action: detail.paused_by_owner ? "resume" : "pause",
    ...(detail.auto_pause_reason ? { autoReason: detail.auto_pause_reason } : {}),
  };
}

/** `EngineSetPaused` on the daemon, then a fresh discovery so the dialog follows. */
export async function setEnginePaused(paused: boolean): Promise<EngineStatusInfo> {
  const status = await invoke<EngineStatusInfo>("daemon_engine_set_paused", { paused });
  await discoverDaemon().catch(() => null);
  return status;
}

export interface HelloInfo {
  daemon_version: string;
  ipc_version: number;
  cloud_url: string;
  account?: { user_id?: string; email?: string; cloud_url: string };
  workspaces: { slug: string; signed_in: boolean; runners: number }[];
  machine_id?: string;
  engine: EngineState;
  engine_detail?: EngineStatusInfo;
}

export interface CliInfo {
  installed: boolean;
  path: string | null;
  version: string | null;
  updatable: boolean;
  update_hint?: string;
}

export type Discovery =
  | { kind: "no_daemon"; cli: CliInfo }
  | { kind: "too_old"; daemon_version?: string; ipc_version?: number; cli: CliInfo }
  | { kind: "app_too_old"; daemon_version: string; ipc_version: number }
  | { kind: "signed_out"; hello: HelloInfo }
  | { kind: "account_mismatch"; daemon_account: string; daemon_cloud_url: string; hosts_runners: boolean }
  | { kind: "compatible"; hello: HelloInfo };

export interface EngineSettings {
  task_root: string;
  task_root_is_default: boolean;
  warnings?: string[];
}

export interface LocalRunner {
  name: string;
  runner_id: string;
  workspace_slug: string | null;
  project_slug: string | null;
  working_dir: string;
  /** `agent.kind` of the block: `engine` for the built-in engine. */
  agent: string;
}

export interface EngineRunnerInfo {
  runner: string;
  runner_id: string;
  workspace: string;
  project: string;
  working_dir: string;
}

// --- Native and server access -----------------------------------------------

type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

export function isDesktopHost(): boolean {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<T>(command, args);
}

class DaemonAPI extends APIService {}
const daemonAPI = new DaemonAPI(API_BASE_URL);

/** A session-authenticated write, with the CSRF token the API requires. */
async function sessionWrite<T>(method: "POST" | "PATCH", path: string, body: unknown): Promise<T> {
  const csrf: { data: { csrf_token: string } } = await daemonAPI.request({ url: CSRF_TOKEN_PATH, method: "GET" });
  const response: { data: T } = await daemonAPI.request({
    url: path,
    method,
    headers: { "Content-Type": "application/json", "X-CSRFTOKEN": csrf.data.csrf_token },
    data: body,
  });
  return response.data;
}

export function sessionPost<T>(path: string, body: unknown): Promise<T> {
  return sessionWrite<T>("POST", path, body);
}

export function sessionPatch<T>(path: string, body: unknown): Promise<T> {
  return sessionWrite<T>("PATCH", path, body);
}

/** A session-authenticated GET. */
export async function sessionGet<T>(path: string, params?: Record<string, string>): Promise<T> {
  const response: { data: T } = await daemonAPI.request({ url: path, method: "GET", params });
  return response.data;
}

export const DEVICE_START_PATH = "/api/v1/auth/device/start/";
export const DEVICE_APPROVE_PATH = "/api/v1/auth/device/approve/";

/**
 * Start a device grant and approve it with the app's session — both halves of
 * the web `/auth/device/` page. The approved code is a bearer credential: it
 * goes to the CLI on stdin and nowhere else.
 *
 * `interactive` marks an approval the user asked for (an explicit engine
 * sign-in); the server refuses a non-interactive engine approval for a machine
 * whose engine sign-in state is `signed_out` or `revoked`.
 */
export async function approveDeviceGrant(options: {
  scope?: "engine";
  workspaceSlug?: string;
  interactive: boolean;
}): Promise<string> {
  const grant = await sessionPost<{ device_code: string; user_code: string }>(
    DEVICE_START_PATH,
    options.scope ? { scope: options.scope } : {}
  );
  await sessionPost(DEVICE_APPROVE_PATH, {
    user_code: grant.user_code,
    ...(options.workspaceSlug ? { workspace_slug: options.workspaceSlug } : {}),
    interactive: options.interactive,
  });
  return grant.device_code;
}

// --- What the app knows about the daemon ------------------------------------

export interface AppIdentity {
  userId: string;
  email?: string;
  cloudUrl: string;
}

let identity: AppIdentity | null = null;
let discovery: Discovery | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

/** Subscribe to discovery changes; returns the unsubscribe. */
export function subscribeDiscovery(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The last discovery, or `null` before the first one (or off the desktop). */
export function currentDiscovery(): Discovery | null {
  return discovery;
}

/** The signed-in app account discovery is evaluated for. */
export function setAppIdentity(next: AppIdentity | null) {
  if (next?.userId !== identity?.userId) {
    discovery = null;
    emit();
  }
  identity = next;
}

export function appIdentity(): AppIdentity | null {
  return identity;
}

/** Ask the host to find the daemon again and classify it for the app's account. */
export async function discoverDaemon(): Promise<Discovery | null> {
  if (!isDesktopHost() || !identity) return null;
  const current = identity;
  const next = await invoke<Discovery>("daemon_discover", { appIdentity: current });
  // A sign-out or account switch meanwhile makes this answer stale.
  if (identity !== current) return discovery;
  discovery = next;
  emit();
  return next;
}

/** The daemon's `Hello` when the app may cooperate with it, else `null`. */
export function compatibleHello(d: Discovery | null = discovery): HelloInfo | null {
  return d?.kind === "compatible" ? d.hello : null;
}

/** The engine's sign-in state the daemon reports, when it reports one. */
export function engineSignInState(hello: HelloInfo | null): EngineSignInState | undefined {
  return hello?.engine_detail?.sign_in_state;
}

/** The account key the daemon keys its engine home by: user id, else e-mail. */
export function daemonAccountKey(hello: HelloInfo | null): string | undefined {
  return hello?.account?.user_id || hello?.account?.email || undefined;
}

/**
 * Why the app's engine features are off, in the user's words; `null` when
 * the daemon is compatible. Every other part of the app keeps working.
 */
export function discoveryProblem(d: Discovery | null): string | null {
  if (!d) return "Looking for Pi Dash on this computer…";
  switch (d.kind) {
    case "compatible":
      return null;
    case "no_daemon":
      return "Pi Dash is not set up on this computer yet.";
    case "too_old":
      return "Pi Dash on this computer is too old for this app. Update it to use the built-in engine.";
    case "app_too_old":
      return `Pi Dash on this computer (${d.daemon_version}) is newer than this app understands. Update the app.`;
    case "signed_out":
      return "Pi Dash on this computer is signed out.";
    case "account_mismatch":
      return `Pi Dash on this computer is signed in as ${d.daemon_account}.`;
  }
}

// --- Engine sign-in rules (design §4.3, §4.6) -------------------------------

/**
 * What the app may do about the engine's sign-in, by the state the daemon
 * reports:
 *
 * | state                  | action           |
 * | ---------------------- | ---------------- |
 * | `expiring`, `expired`  | `renew_silently` |
 * | `never`                | `turn_on` (one click, explicit) |
 * | `signed_out`, `revoked`| `sign_in` (explicit only)       |
 * | `active`, unknown      | `none`           |
 */
export type EngineAction = "renew_silently" | "turn_on" | "sign_in" | "none";

export function engineActionFor(state: EngineSignInState | undefined): EngineAction {
  switch (state) {
    case "expiring":
    case "expired":
      return "renew_silently";
    case "never":
      return "turn_on";
    case "signed_out":
    case "revoked":
      return "sign_in";
    default:
      return "none";
  }
}

/** Approve an engine-only grant with the session and hand it to the daemon's CLI. */
async function signInEngine(interactive: boolean): Promise<void> {
  const deviceCode = await approveDeviceGrant({ scope: "engine", interactive });
  await invoke<void>("pidash_cli_engine_login", { deviceCode });
  await discoverDaemon().catch(() => null);
}

const SILENT_RETRY_MS = 10 * 60 * 1000;
let lastSilentAttempt = 0;
let silentInFlight: Promise<boolean> | null = null;

/**
 * Renew the engine credential with no prompt — only when the daemon is the
 * app's and reports `expiring` or `expired`. Resolves whether it renewed.
 * Never throws: a failed renewal leaves the engine as it was and is retried
 * after a pause rather than on every check.
 */
export function renewEngineIfAllowed(now = Date.now()): Promise<boolean> {
  const hello = compatibleHello();
  if (engineActionFor(engineSignInState(hello)) !== "renew_silently") return Promise.resolve(false);
  if (silentInFlight) return silentInFlight;
  if (now - lastSilentAttempt < SILENT_RETRY_MS) return Promise.resolve(false);
  lastSilentAttempt = now;
  silentInFlight = signInEngine(false)
    .then(
      () => true,
      () => false
    )
    .finally(() => {
      silentInFlight = null;
    });
  return silentInFlight;
}

/** "Turn on the built-in engine" (`never`) or "Sign in the built-in engine"
 * (`signed_out`, `revoked`): an explicit sign-in the user clicked. */
export async function signInEngineExplicitly(): Promise<void> {
  if (!compatibleHello()) throw new Error(discoveryProblem(discovery) ?? "Pi Dash is not available.");
  await signInEngine(true);
}

/** "Sign out the built-in engine": `pidash auth logout --engine`. The app then
 * never renews it silently (the state becomes `signed_out`). */
export async function signOutEngine(): Promise<void> {
  await invoke<void>("pidash_cli_logout", { engine: true });
  await discoverDaemon().catch(() => null);
}

/** Sign the daemon in to one more workspace with an app-approved grant
 * (design §4.9), then have the daemon re-read its tokens. */
export async function signInWorkspace(workspaceSlug: string, options: { force?: boolean } = {}): Promise<void> {
  const deviceCode = await approveDeviceGrant({ workspaceSlug, interactive: false });
  await invoke<void>("pidash_cli_login", {
    deviceCode,
    cloudUrl: API_BASE_URL,
    workspace: workspaceSlug,
    force: options.force ?? false,
  });
  await invoke<void>("daemon_credentials_reload").catch(() => {});
  await discoverDaemon().catch(() => null);
}

/** Test-only: forget everything this module remembered. */
export function resetDaemonStateForTests() {
  identity = null;
  discovery = null;
  lastSilentAttempt = 0;
  silentInFlight = null;
  listeners.clear();
}
