// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Local agent-runtime service (RUN-044, RUN-045, RUN-047; PIDESKAPP-222).
//
// The app owns no daemon, runner or engine: this computer's Pi Dash daemon
// — the command-line tool's, run as a user service — hosts them, and the
// app is its client (`.ai_design/built_engine_as_runner/design.md` §4.1).
// So nothing here enrols a machine when a project opens, writes engine
// config or model tokens, or starts and stops a process. What is left:
//
// - the signed-in account, which scopes local chat history and is what
//   discovery classifies the daemon against;
// - discovery, and setting the daemon up when there is none — only after
//   the user accepted the consent screen (§4.7);
// - keeping the engine signed in: signed in or renewed silently from every
//   state but `revoked`, with no engine sign-in of its own
//   (desktop_unified_signin §6.3);
// - "Run on this computer": turning the Pi Dash Agent on for a project
//   the first time a run uses it (§5, enrol on use);
// - the app half of a sign-out: approval modes stashed, chat history
//   deleted only when asked; the computer signs out with the app
//   (desktop_unified_signin §4).
//
// Every entry takes the API client plus the platform capability and is
// inert without the capability, so web and cloud builds make no call.

import {
  approveDeviceCode,
  getPidashAgentProfile,
  getIssue,
  getProject,
  startDeviceGrant,
  type ApiClient,
} from "@pidash/api-client";

import type {
  AgentRuntimeApi,
  CliSetupApi,
  DaemonDiscovery,
  DaemonPidashAgentRunner,
  DaemonPidashAgentSignInState,
  DaemonHello,
  DaemonLocalRunner,
  LocalChatApi,
} from "../../../core/platform/index.js";
import type { RuntimeSignOutChoice } from "../../../core/session/index.js";
import { agentReasonMessage } from "../../../shared/lib/agentReasons.js";
import { restoreApprovalModes, stashApprovalModes } from "./approvalModes.js";
import { requestDaemonSetup, type SetUpReason } from "./setupPrompt.js";
import { sameCloud } from "./sync.js";

/** Server executor kind for the Pi Dash Agent. */
export const PIDASH_AGENT_EXECUTOR_KIND = "managed_runner";

/** Heartbeat wait after preparing a run: attempts and pause between them. */
export const HEARTBEAT_POLL_ATTEMPTS = 30;
export const HEARTBEAT_POLL_INTERVAL_MS = 1000;

/** A failed silent renewal is retried after this pause, not on every check. */
export const SILENT_RENEW_RETRY_MS = 10 * 60 * 1000;

/** Most often the daemon may be woken for a reconnect: focus events and the
 * minute tick would otherwise retry a dead cloud connection on every wake
 * during a long outage, defeating the daemon's backoff. */
export const RECONNECT_MIN_INTERVAL_MS = 30_000;

export const PIDASH_AGENT_STILL_CONNECTING_TEXT =
  "The Pi Dash Agent on this computer has not connected yet. Try Run again shortly.";

/** The daemon reports the engine unavailable; carries its reason code. */
export class PidashAgentUnavailableError extends Error {
  readonly code: string;

  constructor(code: string) {
    super(agentReasonMessage(code));
    this.name = "PidashAgentUnavailableError";
    this.code = code;
  }
}

/** What a user-triggered agent run targets. */
export interface AgentRunTarget {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

/** Overrides for the heartbeat wait (tests shorten it). */
export interface HeartbeatPoll {
  maxAttempts?: number | undefined;
  intervalMs?: number | undefined;
  sleep?: ((ms: number) => Promise<void>) | undefined;
}

export interface RuntimeDeps {
  client: ApiClient;
  runtime?: AgentRuntimeApi | undefined;
}

interface Identity {
  userId: string;
  email?: string | undefined;
  cloudUrl: string;
}

interface RuntimeState {
  /** The account local history is scoped to; kept through sign-out until the page goes. */
  account: string;
  /** Who discovery is evaluated for; `null` before resume and after sign-out. */
  identity: Identity | null;
  discovery: DaemonDiscovery | null;
  lastSilentAttempt: number;
  silentInFlight: Promise<boolean> | null;
  /** Last reconnect wake, in epoch milliseconds (`Date.now()` basis). */
  lastReconnectWake: number;
  /** Where the user adds model credit, from the last agent profile the server read. */
  topUpUrl: string;
}

const state: RuntimeState = {
  account: "",
  identity: null,
  discovery: null,
  lastSilentAttempt: 0,
  silentInFlight: null,
  lastReconnectWake: 0,
  topUpUrl: "",
};

/** Drop all runtime state. Tests only; the app resumes instead. */
export function resetRuntimeServiceForTests(): void {
  state.account = "";
  state.identity = null;
  state.discovery = null;
  state.lastSilentAttempt = 0;
  state.silentInFlight = null;
  state.lastReconnectWake = 0;
  state.topUpUrl = "";
}

/**
 * Wake the daemon's cloud connections, rate-limited (PIDESKAPP-181). After a
 * long outage the daemon's retry backoff can be hours long; a user action
 * (focus, network recovery, Run) cuts it short so the built-in agent comes
 * back within seconds. Best-effort: never throws.
 */
function wakeDaemonForReconnect(runtime: AgentRuntimeApi): void {
  const now = Date.now();
  if (now - state.lastReconnectWake < RECONNECT_MIN_INTERVAL_MS) return;
  state.lastReconnectWake = now;
  void runtime.requestReconnect().catch(() => undefined);
}

/**
 * The signed-in account id, set by resumeRuntime. Local chat history is
 * scoped per account, so the chat transport keys its store by this value.
 * Empty until the runtime has resumed for a user (always on web).
 */
export function getRuntimeAccount(): string {
  return state.account;
}

/**
 * Where the user adds model credit, from the last agent profile the
 * server read. Empty when the server names no place.
 */
export function getModelTopUpUrl(): string {
  return state.topUpUrl;
}

/** Best-effort read of the top-up page off the agent profile (chat transcript link). */
async function refreshTopUpUrl(deps: RuntimeDeps): Promise<void> {
  const { client, runtime } = deps;
  if (!runtime || !state.identity) return;
  try {
    const profile = await getPidashAgentProfile(client);
    state.topUpUrl = profile.top_up_url ?? "";
  } catch {
    // Keep what we had.
  }
}

/** The last discovery, or `null` before the first one (or off the desktop). */
export function currentDiscovery(): DaemonDiscovery | null {
  return state.discovery;
}

/** The daemon's `Hello` when the app may cooperate with it, else `null`. */
export function compatibleHello(discovery: DaemonDiscovery | null = state.discovery): DaemonHello | null {
  return discovery?.kind === "compatible" ? discovery.hello : null;
}

/** The account key the daemon keys its engine home by: user id, else e-mail. */
export function daemonAccountKey(hello: DaemonHello | null): string | undefined {
  return hello?.account?.user_id ?? hello?.account?.email ?? undefined;
}

/**
 * Why the app's engine features are off, in the user's words; `null` when
 * the daemon is compatible. Every other part of the app keeps working.
 */
export function discoveryProblem(discovery: DaemonDiscovery | null, appCloudUrl?: string | undefined): string | null {
  if (!discovery) return "Looking for Pi Dash on this computer…";
  switch (discovery.kind) {
    case "compatible":
      return null;
    case "no_daemon":
      return "Pi Dash is not set up on this computer yet.";
    case "too_old":
      return "Pi Dash on this computer is too old for this app. Update it to use the Pi Dash Agent.";
    case "app_too_old":
      return `Pi Dash on this computer (${discovery.daemon_version}) is newer than this app understands. Update the app.`;
    case "not_answering":
      return "Pi Dash on this computer is not answering right now. The app will try again.";
    case "signed_out":
      return "Pi Dash on this computer is signed out.";
    case "account_mismatch": {
      // A daemon on another cloud: the app cannot follow (§6.2), so the
      // notice names the cloud instead of the account.
      if (appCloudUrl !== undefined && !sameCloud(appCloudUrl, discovery.daemon_cloud_url)) {
        return `Pi Dash on this computer is signed in to ${discovery.daemon_cloud_url}.`;
      }
      return `Pi Dash on this computer is signed in as ${discovery.daemon_account}.`;
    }
  }
}

/**
 * What the app may do about the engine's sign-in, by the state the daemon
 * reports (desktop_unified_signin §4, §6.3): the engine has no sign-in of
 * its own, so `never`, `signed_out`, `expiring` and `expired` are all
 * signed in or renewed silently while the app is signed in. `revoked`
 * still needs an interactive sign-in — which the app never offers — so the
 * daemon reports the engine unavailable instead of the app asking.
 */
export type PidashAgentAction = "renew_silently" | "none";

export function pidashAgentActionFor(signIn: DaemonPidashAgentSignInState | undefined): PidashAgentAction {
  switch (signIn) {
    case "expiring":
    case "expired":
    case "never":
    case "signed_out":
      return "renew_silently";
    default:
      return "none";
  }
}

function pidashAgentSignInState(hello: DaemonHello | null): DaemonPidashAgentSignInState | undefined {
  return hello?.engine_detail?.sign_in_state;
}

/**
 * Ask the shell to find the daemon again and classify it for the app's
 * account. An answer that arrives after a sign-out or account switch is
 * stale and dropped. Off the desktop, or before resume, resolves `null`.
 */
export async function discoverDaemon(runtime: AgentRuntimeApi | undefined): Promise<DaemonDiscovery | null> {
  const identity = state.identity;
  if (!runtime || !identity) return null;
  const next = await runtime.discover(identity);
  if (state.identity !== identity) return state.discovery;
  state.discovery = next;
  return next;
}

/** Discover, keeping the last answer when the shell call itself fails. */
async function rediscover(runtime: AgentRuntimeApi): Promise<DaemonDiscovery | null> {
  try {
    return await discoverDaemon(runtime);
  } catch {
    return state.discovery;
  }
}

/** Start an engine-only grant and approve it with the app's session. */
async function approvePidashAgentGrant(client: ApiClient, interactive: boolean): Promise<string> {
  const grant = await startDeviceGrant(client, { scope: "engine" });
  await approveDeviceCode(client, grant.user_code, { interactive });
  return grant.device_code;
}

async function signInPidashAgent(
  deps: { client: ApiClient; runtime: AgentRuntimeApi },
  interactive: boolean
): Promise<void> {
  const deviceCode = await approvePidashAgentGrant(deps.client, interactive);
  await deps.runtime.pidashAgentLogin(deviceCode);
  await rediscover(deps.runtime);
}

/**
 * Renew the engine credential with no prompt — only when the daemon is the
 * app's and reports `expiring` or `expired`. Resolves whether it renewed.
 * Never throws: a failed renewal leaves the engine as it was and is retried
 * after a pause rather than on every check.
 */
export function renewPidashAgentIfAllowed(deps: RuntimeDeps, now: number = Date.now()): Promise<boolean> {
  const { client, runtime } = deps;
  if (!runtime) return Promise.resolve(false);
  if (pidashAgentActionFor(pidashAgentSignInState(compatibleHello())) !== "renew_silently")
    return Promise.resolve(false);
  if (state.silentInFlight) return state.silentInFlight;
  if (now - state.lastSilentAttempt < SILENT_RENEW_RETRY_MS) return Promise.resolve(false);
  state.lastSilentAttempt = now;
  const attempt = signInPidashAgent({ client, runtime }, false)
    .then(
      () => true,
      () => false
    )
    .finally(() => {
      state.silentInFlight = null;
    });
  state.silentInFlight = attempt;
  return attempt;
}

/**
 * Resume the runtime for the signed account: discovery is evaluated for it
 * (a daemon signed in as someone else is refused), and the approval modes
 * stashed at the last sign-out come back. Inert without the capability.
 */
export function resumeRuntime(
  user: { id: string | undefined; email?: string | undefined; cloudUrl: string },
  deps: RuntimeDeps
): void {
  const { runtime } = deps;
  if (!runtime || !user.id) return;
  const current = state.identity;
  if (current?.userId === user.id && current.email === user.email && current.cloudUrl === user.cloudUrl) return;
  if (current?.userId !== user.id) {
    state.discovery = null;
    state.lastSilentAttempt = 0;
    state.topUpUrl = "";
  }
  if (state.account !== user.id) {
    state.account = user.id;
    // A sign-out wiped the webview's storage; put this account's approval
    // modes back before the user can reach a chat.
    void restoreApprovalModes({ account: user.id, runtime }).catch(() => undefined);
  }
  state.identity = { userId: user.id, email: user.email, cloudUrl: user.cloudUrl };
}

/**
 * Re-check the daemon, wake its cloud connections, and keep the engine signed
 * in: called on resume, on focus, on network recovery and every minute.
 * Silent renewal happens only in the two states that allow it. Never throws.
 */
export async function refreshRuntime(deps: RuntimeDeps): Promise<void> {
  const { runtime } = deps;
  if (!runtime || !state.identity) return;
  await rediscover(runtime);
  wakeDaemonForReconnect(runtime);
  await renewPidashAgentIfAllowed(deps);
  await refreshTopUpUrl(deps);
}

/** What the set-up does, step by step, for its progress screen. */
export type DaemonSetUpStep = "install_cli" | "sign_in" | "service" | "engine" | "done";

/**
 * Set Pi Dash up on this computer, through the command-line tool's own
 * steps so the result is a terminal install: install the tool the app
 * carries, sign it in to the workspace with a grant the app's session
 * approves (skipped when the daemon is already this account's), install
 * and start the user service, install the engine. Run only after the user
 * accepted the consent screen.
 */
export async function setUpDaemon(
  input: {
    workspaceSlug: string;
    cli: CliSetupApi | undefined;
    onStep?: ((step: DaemonSetUpStep) => void) | undefined;
  } & RuntimeDeps
): Promise<void> {
  const { client, runtime, cli, workspaceSlug } = input;
  const onStep = input.onStep ?? (() => undefined);
  const identity = state.identity;
  if (!runtime || !identity) throw new Error("The local agent is signed out.");
  if (!cli) throw new Error("This build cannot set up Pi Dash on this computer.");
  onStep("install_cli");
  const status = await cli.detect();
  if (!status.installed) await cli.install();
  if (!compatibleHello(await rediscover(runtime))) {
    onStep("sign_in");
    const grant = await startDeviceGrant(client);
    await approveDeviceCode(client, grant.user_code, { interactive: false, workspaceSlug });
    // The tool finishes the exchange and owns everything it writes.
    await cli.login({ deviceCode: grant.device_code, cloudUrl: identity.cloudUrl, workspace: workspaceSlug });
  }
  onStep("service");
  await runtime.installService();
  onStep("engine");
  await runtime.ensurePidashAgent();
  await discoverDaemon(runtime);
  onStep("done");
}

/**
 * Sign the daemon in to one more workspace with an app-approved grant
 * (design §4.9), then have the daemon re-read its tokens. The migration
 * and the replace flow need this when the daemon does not yet hold the
 * workspace's credential.
 */
export async function signInWorkspace(
  input: { workspaceSlug: string; cli: CliSetupApi | undefined } & RuntimeDeps
): Promise<void> {
  const { client, runtime, cli, workspaceSlug } = input;
  const identity = state.identity;
  if (!runtime || !identity) throw new Error("The local agent is signed out.");
  if (!cli) throw new Error("This build cannot sign Pi Dash on this computer in to that workspace.");
  const grant = await startDeviceGrant(client);
  await approveDeviceCode(client, grant.user_code, { interactive: false, workspaceSlug });
  // The tool finishes the exchange and owns everything it writes.
  await cli.login({ deviceCode: grant.device_code, cloudUrl: identity.cloudUrl, workspace: workspaceSlug });
  await runtime.reloadCredentials().catch(() => undefined);
  await rediscover(runtime);
}

/**
 * Sign the computer's daemon in to this account with no workspace named
 * (desktop_unified_signin §4): the server auto-picks the grant's workspace
 * and the tool picks the one it serves. The new account's silent sign-in
 * signs the daemon — and the engine with it (§6.3) — in at once, switching
 * accounts when the daemon is signed in as someone else (§6.4).
 * `force` rebinds a daemon on another cloud to the app's cloud (§6.2).
 */
export async function signInDaemonForAccount(
  input: { cli: CliSetupApi | undefined; force?: boolean | undefined } & RuntimeDeps
): Promise<void> {
  const { client, runtime, cli, force } = input;
  const identity = state.identity;
  if (!runtime || !identity) throw new Error("The local agent is signed out.");
  if (!cli) throw new Error("This build cannot sign Pi Dash on this computer in.");
  const grant = await startDeviceGrant(client);
  await approveDeviceCode(client, grant.user_code, { interactive: false });
  // The tool finishes the exchange and owns everything it writes; an
  // account switch parks and restores identities and restarts the daemon.
  await cli.login({ deviceCode: grant.device_code, cloudUrl: identity.cloudUrl, workspace: "", force });
  await runtime.reloadCredentials().catch(() => undefined);
  await rediscover(runtime);
}

/**
 * A daemon the app can use, set up on the user's say-so when there is
 * none. Throws the reason when there is still none.
 */
export async function ensureDaemon(
  reason: SetUpReason,
  workspaceSlug: string,
  deps: RuntimeDeps
): Promise<DaemonHello> {
  const { runtime } = deps;
  if (!runtime) throw new Error("Pi Dash is not available on this build.");
  const found = await rediscover(runtime);
  const ready = compatibleHello(found);
  if (ready) return ready;
  if (found?.kind === "no_daemon" || found?.kind === "signed_out") {
    if (await requestDaemonSetup({ reason, workspaceSlug })) {
      const after = compatibleHello();
      if (after) return after;
    }
  }
  throw new Error(
    discoveryProblem(state.discovery, state.identity?.cloudUrl) ?? "Pi Dash is not set up on this computer."
  );
}

/** Refuse when the engine cannot take work, with the sentence the user needs. */
function assertPidashAgentUsable(hello: DaemonHello): void {
  if (hello.engine === "unavailable") {
    throw new PidashAgentUnavailableError(hello.engine_detail?.reason_code ?? "");
  }
  // The engine has no sign-in of its own (desktop_unified_signin U-3): a
  // revoked engine is not something the app can sign in, so the daemon's
  // unavailability is the story, never an ask to sign in.
  if (pidashAgentSignInState(hello) === "revoked") {
    throw new PidashAgentUnavailableError(hello.engine_detail?.reason_code ?? "");
  }
}

/**
 * Make the daemon ready for a local chat: set it up if there is none
 * (asking first), renew the engine credential when that is allowed, and
 * refuse with the reason otherwise. Inert without the capability.
 */
export async function ensureChatRuntime(input: { workspaceSlug: string } & RuntimeDeps): Promise<void> {
  const { runtime, workspaceSlug } = input;
  if (!runtime) return;
  await ensureDaemon("chat", workspaceSlug, input);
  await renewPidashAgentIfAllowed(input);
  const hello = compatibleHello();
  if (!hello)
    throw new Error(
      discoveryProblem(state.discovery, state.identity?.cloudUrl) ?? "Pi Dash is not set up on this computer."
    );
  assertPidashAgentUsable(hello);
}

/**
 * The model gateway refused the engine's credential mid-chat. The daemon
 * owns model tokens now, so the app can only renew the engine sign-in —
 * silently, when its state allows. Rejects when it cannot; the caller asks
 * the user to sign in.
 */
export async function refreshModelCredential(deps: RuntimeDeps): Promise<void> {
  const { runtime } = deps;
  if (!runtime) return;
  await rediscover(runtime);
  if (!(await renewPidashAgentIfAllowed(deps))) throw new Error("The Pi Dash Agent needs a new sign-in.");
}

/** The engine runner this computer has for a project, if any. */
export async function localPidashAgentRunner(
  runtime: AgentRuntimeApi,
  workspaceSlug: string,
  project: string
): Promise<DaemonLocalRunner | undefined> {
  const runners = await runtime.localRunners();
  return runners.find(
    (runner) => runner.agent === "engine" && runner.workspace_slug === workspaceSlug && runner.project_slug === project
  );
}

/**
 * Turn the Pi Dash Agent on for a project on this computer, unless it
 * already is (design §5). Sets the daemon up first when there is none.
 */
export async function turnOnPidashAgentForProject(
  input: { workspaceSlug: string; project: string; workingDir?: string | undefined } & RuntimeDeps
): Promise<DaemonPidashAgentRunner | DaemonLocalRunner> {
  const { runtime, workspaceSlug, project, workingDir } = input;
  if (!runtime) throw new Error("Pi Dash is not available on this build.");
  await ensureDaemon("engine", workspaceSlug, input);
  const existing = await localPidashAgentRunner(runtime, workspaceSlug, project);
  if (existing) return existing;
  return runtime.addPidashAgentRunner({ workspace: workspaceSlug, project, workingDir });
}

/**
 * Prepare a user-triggered agent run. Only when the work item targets the
 * Pi Dash Agent (explicit pin, else the project default): wake the
 * daemon's cloud connections (PIDESKAPP-181 — the Run click is a user
 * action, so an outage's multi-hour backoff must not outlast it), turn the
 * engine on for that project on this computer — the first run is what
 * enrols it — and wait briefly for the server to see it online. Any
 * other executor is left alone; selection is never mutated here.
 */
export async function prepareAgentRun(
  input: { target: AgentRunTarget; heartbeatPoll?: HeartbeatPoll | undefined } & RuntimeDeps
): Promise<void> {
  const { client, runtime, target } = input;
  if (!runtime) return;
  const { workspaceSlug, projectId, issueId } = target;
  const [issue, project] = await Promise.all([
    getIssue(client, workspaceSlug, projectId, issueId),
    getProject(client, workspaceSlug, projectId),
  ]);
  if ((issue.agent_executor ?? project.default_agent_executor) !== PIDASH_AGENT_EXECUTOR_KIND) return;
  wakeDaemonForReconnect(runtime);
  await turnOnPidashAgentForProject({ client, runtime, workspaceSlug, project: project.identifier });
  const attempts = input.heartbeatPoll?.maxAttempts ?? HEARTBEAT_POLL_ATTEMPTS;
  const intervalMs = input.heartbeatPoll?.intervalMs ?? HEARTBEAT_POLL_INTERVAL_MS;
  const sleep = input.heartbeatPoll?.sleep ?? defaultSleep;
  for (let attempt = 0; attempt < attempts; attempt++) {
    const current = await getProject(client, workspaceSlug, projectId);
    if (
      current.agent_executor_options.some((option) => option.kind === PIDASH_AGENT_EXECUTOR_KIND && option.available)
    ) {
      return;
    }
    await sleep(intervalMs);
  }
  throw new Error(PIDASH_AGENT_STILL_CONNECTING_TEXT);
}

function defaultSleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Release the runtime for an app-only account switch (desktop_unified_signin
 * §4 row 6: the CLI signed in as someone else, so the app signs its current
 * account out locally and follows). Unlike `disposeRuntimeForSignOut` the
 * computer stays signed in — the daemon is the side being followed — and no
 * sign-out event is recorded. The approval modes are stashed for the old
 * account and the runtime identity is dropped, so `resumeRuntime` starts
 * clean for the new one. Never rejects.
 */
export async function releaseRuntimeForAccountSwitch(input: Pick<RuntimeDeps, "runtime">): Promise<void> {
  const { runtime } = input;
  if (!runtime) return;
  const account = getRuntimeAccount();
  try {
    await stashApprovalModes({ account, runtime });
  } catch {
    // A lost preference must not block the switch.
  }
  state.identity = null;
  state.discovery = null;
  state.topUpUrl = "";
  state.lastSilentAttempt = 0;
  state.account = "";
}

/**
 * The app half of a sign-out. The daemon, the engine and every runner stay
 * signed in (they are independent of the app) unless the user ticked "Also
 * sign out Pi Dash on this computer". Approval modes are handed to the host
 * before the webview wipe; chat history goes only when the dialog asked for
 * it. Never rejects.
 */
export async function disposeRuntimeForSignOut(
  input: { choice: RuntimeSignOutChoice; chat?: Pick<LocalChatApi, "clearHistory"> | undefined } & RuntimeDeps
): Promise<void> {
  const { runtime, choice, chat } = input;
  if (!runtime) return;
  const account = getRuntimeAccount();
  try {
    await stashApprovalModes({ account, runtime });
  } catch {
    // Signing out matters more than keeping a preference; wipe regardless.
  }
  if (choice.deleteChatHistory && account && chat) {
    try {
      await chat.clearHistory(account);
    } catch {
      // Deleting history failed; the sign-out itself still goes ahead.
    }
  }
  // Unified sign-in (desktop_unified_signin §4): the app's sign-out always
  // signs the computer out. When the server cannot be reached the logout
  // runs `--local-only`, so the computer is signed out regardless. A failed
  // logout is retried by the sync supervisor, whose sign-out is the newest
  // event (§6.2).
  try {
    await runtime.signOutComputer();
  } catch {
    try {
      await runtime.signOutComputer({ localOnly: true });
    } catch {
      // Even the local clear failed; the sync supervisor retries the logout.
    }
  }
  // The app's own sign-out event (design §6.2): the newest event wins, so
  // a CLI logout that failed above is retried, never undone. Written in the
  // host's store, which survives the webview wipe that follows.
  try {
    const prior = await runtime.getAuthSyncRecord().catch(() => null);
    await runtime.setAuthSyncRecord({
      own_event: {
        kind: "signed_out",
        account: {
          user_id: account || undefined,
          email: state.identity?.email,
          cloud_url: state.identity?.cloudUrl ?? "",
        },
        at: new Date().toISOString(),
      },
      last_seen_seq: prior?.last_seen_seq ?? null,
    });
  } catch {
    // The record is best-effort: the supervisor re-derives it on the next check.
  }
  // The account stays readable until the page navigates away; discovery
  // is forgotten so nothing acts on the daemon for the old session (an
  // answer still in flight is dropped as stale).
  state.identity = null;
  state.discovery = null;
  state.topUpUrl = "";
}
