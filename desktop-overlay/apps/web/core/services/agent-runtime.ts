/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@pi-dash/services";
import { AGENT_RUNTIME_REASON_MESSAGES, CSRF_TOKEN_PATH } from "@/pi-dash-web/components/desktop/agent-runtime-edition";
import { restoreApprovalModes } from "@/services/desktop-approval-modes";
import type { DaemonStatus } from "@/services/desktop-account-sync";
import {
  activeDaemonRuns,
  CLI_OUTDATED_MESSAGE,
  cliErrorCode,
  cliErrorMessage,
  daemonUserUnknown,
  getDaemonAuthStatus,
  observeDesktopAuth,
  reportDaemonStatus,
  signDaemonIn,
  signDaemonOut,
} from "@/services/desktop-account-sync";

// Shares the SPA's axios setup — including any edition 401 -> refresh ->
// retry interceptor — so a normal access-cookie expiry does not revoke the
// agent where the edition can refresh it.
class AgentAPI extends APIService {}
const agentAPI = new AgentAPI(API_BASE_URL);

export type AgentRunTarget = { workspaceSlug: string; projectId: string; issueId: string };
type Profile = { available: boolean; reason_code: string; base_url: string; model: string; top_up_url?: string };
type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };
/** What the user chose in the sign-out dialog. */
export type SignOutChoice = { deleteChatHistory: boolean };
/** What the sign-out dialog has to say before asking. */
export type SignOutContext = { activeRuns: number };
type SignOutPrompt = (resolve: (choice: SignOutChoice | null) => void, context: SignOutContext) => void;

export function isDesktop() {
  return typeof window !== "undefined" && "__TAURI__" in window;
}

/**
 * The signed-in account id, set by {@link resumeAgentRuntime}. Local chat
 * history is scoped per account, so the local chat transport keys its SQLite
 * store by this value. Empty until the runtime has resumed for a user.
 */
export function getAgentAccount(): string {
  return activeUserId;
}

/**
 * The account every engine command is scoped to. The engine keeps its own copy
 * of each conversation, so its home directory is per account; a command with
 * no account has nowhere safe to point it.
 */
function requireAccount(): string {
  if (!activeUserId) throw new Error("Pi Dash Agent is signed out.");
  return activeUserId;
}

let signOutPrompt: SignOutPrompt | null = null;

/**
 * Mounts (or, with `null`, unmounts) the dialog that {@link confirmSignOut}
 * opens. Owned by the `AgentRuntime` component.
 */
export function registerSignOutPrompt(prompt: SignOutPrompt | null) {
  signOutPrompt = prompt;
}

/**
 * Ask the user to confirm a sign-out they started, and whether to delete this
 * account's chat history with it. Resolves `null` when they cancel.
 *
 * Signing the app out signs this computer's `pidash` CLI out too, which stops
 * the shared daemon's runs — so the dialog says how many are in progress.
 *
 * Deletion is opt-in: with no dialog to ask through, sign-out proceeds and
 * keeps the history.
 */
export async function confirmSignOut(): Promise<SignOutChoice | null> {
  if (!isDesktop() || !signOutPrompt) return { deleteChatHistory: false };
  const activeRuns = await activeDaemonRuns();
  const prompt = signOutPrompt;
  if (!prompt) return { deleteChatHistory: false };
  return new Promise((resolve) => prompt(resolve, { activeRuns }));
}

// Set while the app signs out to follow the CLI, which is already signed out
// (or signed in as someone else): signing the CLI out then would undo the very
// change being followed.
let followingCli = false;

/**
 * Sign the app out because the CLI's sign-in or sign-out is the more recent
 * one (account sync). Runs `signOut` — the store's — without signing the CLI
 * out.
 */
export async function signOutFollowingCli(signOut: () => Promise<void>): Promise<void> {
  followingCli = true;
  try {
    await signOut();
  } finally {
    followingCli = false;
  }
}

function invoke<T>(command: string, args?: Record<string, unknown>) {
  return (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<T>(command, args);
}

async function api<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (method !== "GET") {
    const csrf = await api<{ csrf_token: string }>(CSRF_TOKEN_PATH);
    headers["X-CSRFTOKEN"] = csrf.csrf_token;
  }
  try {
    const response = await agentAPI.request({ url: path, method, headers, data: body });
    return response.data;
  } catch (error) {
    const response = (error as { response?: { status: number; data?: { error?: string; detail?: string } } }).response;
    if (response?.status === 401) {
      stopped = true;
      generation++;
      forgetEnrollments();
      tokenExpiresAt = 0;
      await invoke("managed_sign_out");
    }
    const reason = response?.data?.error ?? response?.data?.detail ?? "Could not connect Pi Dash Agent.";
    throw new Error(AGENT_RUNTIME_REASON_MESSAGES[reason] ?? reason, { cause: error });
  }
}

// Serialize config mutations; a project-open effect and a Run click can overlap.
let pending: Promise<unknown> = Promise.resolve();
let generation = 0;
let stopped = false;
// Workspaces with a built-in engine runner enrolled in this sign-in.
const workspaces = new Set<string>();
// The one project each workspace's local chat is enrolled against. Sticky:
// every extra project is another runner in the daemon, so it is chosen once
// and reused rather than re-read from the project list on each message.
const chatProjects = new Map<string, string>();
let tokenExpiresAt = 0;
let hostLabel = "";
let activeUserId = "";
// Workspaces whose local chat is fully provisioned in this page, each stamped
// with the sign-in and host it was provisioned under. Deliberately not
// persisted: a reload provisions once more, then is back on the fast path.
const chatReady = new Map<string, string>();

function chatStamp() {
  return `${generation}:${activeUserId}:${hostLabel}`;
}
let topUpUrl = "";

function enrollmentKey() {
  return `pidash-managed-workspaces:${activeUserId}`;
}

function saveEnrollmentCache() {
  if (!activeUserId) return;
  try {
    sessionStorage.setItem(
      enrollmentKey(),
      JSON.stringify({ hostLabel, workspaces: [...workspaces], chatProjects: Object.fromEntries(chatProjects) })
    );
  } catch {
    // Storage can be unavailable; enrollment still works for this page.
  }
}

function clearEnrollmentCache() {
  if (!activeUserId) return;
  try {
    sessionStorage.removeItem(enrollmentKey());
  } catch {
    // Native sign-out still deletes the credentials.
  }
}

function forgetEnrollments() {
  workspaces.clear();
  chatProjects.clear();
  clearEnrollmentCache();
}

function enqueue<T>(action: () => Promise<T>): Promise<T> {
  const next = pending.then(action, action);
  pending = next.catch(() => {});
  return next;
}

export function resumeAgentRuntime(userId?: string) {
  if (userId && userId !== activeUserId) {
    activeUserId = userId;
    workspaces.clear();
    chatProjects.clear();
    try {
      const cached = JSON.parse(sessionStorage.getItem(enrollmentKey()) ?? "null");
      if (typeof cached?.hostLabel === "string" && Array.isArray(cached.workspaces)) {
        hostLabel = cached.hostLabel;
        for (const workspace of cached.workspaces) if (typeof workspace === "string") workspaces.add(workspace);
        for (const [workspace, project] of Object.entries(cached.chatProjects ?? {}))
          if (typeof project === "string") chatProjects.set(workspace, project);
      }
    } catch {
      clearEnrollmentCache();
    }
    // A sign-out wiped the webview's storage; put this account's approval
    // mode back before the user can reach a chat.
    void restoreApprovalModes(userId).catch(() => {});
  }
  stopped = false;
}

async function configure() {
  const profile = await api<Profile>("/api/users/me/ai-assistant/agent-profile/");
  if (!profile.available) {
    // The daemon is shared with the user's own runners, so it keeps running;
    // only the built-in engine stops being offered.
    chatReady.clear();
    throw new Error(AGENT_RUNTIME_REASON_MESSAGES[profile.reason_code] ?? profile.reason_code);
  }
  await invoke("managed_write_engine_config", { account: requireAccount(), profile });
  topUpUrl = profile.top_up_url ?? "";
  if (tokenExpiresAt < Date.now() + 120_000) {
    const credential = await api<{ token: string; expires_at: string }>(
      "/api/users/me/ai-assistant/agent-token/",
      "POST",
      {}
    );
    const expiresAt = Date.parse(credential.expires_at);
    if (!credential.token || !Number.isFinite(expiresAt) || expiresAt <= Date.now()) {
      throw new Error("Pi Dash returned an expired model credential. Sign in again.");
    }
    await invoke("managed_write_model_token", { token: credential.token });
    tokenExpiresAt = expiresAt;
  }
}

/**
 * Where the user adds model credit, as reported by the last agent profile.
 * Empty when the server names no such place.
 */
export function getModelTopUpUrl(): string {
  return topUpUrl;
}

/**
 * Replace the engine's model credential now, whatever its recorded expiry.
 *
 * For a gateway that has just refused the stored one: the file can hold a
 * token the gateway no longer accepts well before `expires_at`. Rejects when
 * no new credential can be had — the caller asks the user to sign in.
 */
export async function refreshModelCredential(): Promise<void> {
  if (!isDesktop()) return;
  const current = generation;
  return enqueue(async () => {
    if (stopped || current !== generation) throw new Error("Pi Dash Agent is signed out.");
    tokenExpiresAt = 0;
    await configure();
  });
}

/**
 * Make sure this computer's `pidash` CLI — whose default config the shared
 * daemon runs from — is signed in as this account and bound to `workspaceSlug`.
 *
 * The built-in engine's runners are registered with the CLI's machine token,
 * which belongs to one account and one workspace. A CLI that is signed out is
 * signed in here; one signed in as someone else is left alone (account sync
 * decides which sign-in wins, by which is more recent).
 */
async function ensureCliSignedIn(workspaceSlug: string): Promise<void> {
  const account = requireAccount();
  const read = () =>
    getDaemonAuthStatus().catch((error: unknown) => {
      throw new Error(
        cliErrorCode(error) === "cli_outdated"
          ? `${CLI_OUTDATED_MESSAGE} It is needed to run the built-in agent.`
          : cliErrorMessage(error),
        { cause: error }
      );
    });
  let status = await read();
  if (!status.signed_in) {
    try {
      await signDaemonIn(workspaceSlug);
    } catch (error) {
      throw new Error(cliErrorMessage(error), { cause: error });
    }
    status = await read();
  }
  if (!status.signed_in) throw new Error("Pi Dash could not sign in the pidash CLI on this computer.");
  // A CLI that cannot name its user may be someone else's sign-in: enrolling
  // this account's runner under it is not ours to decide. Account sync asks
  // the user whether to re-sign it.
  if (daemonUserUnknown(status)) {
    throw new Error(
      "The pidash CLI on this computer is signed in to an account Pi Dash cannot identify. " +
        "Switch it to this account when asked, or sign the CLI in again, to use the built-in agent."
    );
  }
  if (status.user && status.user.id !== account) {
    throw new Error(
      `The pidash CLI on this computer is signed in as ${status.user.email || "another account"}. ` +
        "Sign in to Pi Dash as that account, or sign the CLI out, to use the built-in agent."
    );
  }
  if (status.workspace_slug && status.workspace_slug !== workspaceSlug) {
    throw new Error(
      `Pi Dash Agent on this computer runs in the "${status.workspace_slug}" workspace. ` +
        "Open that workspace to use the built-in agent."
    );
  }
}

/** Point the engine's runners at this account and make sure the daemon runs. */
async function startDaemon(): Promise<void> {
  let status: DaemonStatus;
  try {
    status = await invoke<DaemonStatus>("managed_start_daemon", { account: requireAccount() });
  } catch (error) {
    throw new Error(cliErrorMessage(error), { cause: error });
  }
  reportDaemonStatus(status);
  if (status.mode === "unavailable" || status.mode === "idle") {
    throw new Error(status.message ?? "Pi Dash Agent could not start on this computer.");
  }
}

/** Register the engine's runner for one project in the shared config. */
async function enroll(workspaceSlug: string, project: string, chat: boolean): Promise<void> {
  try {
    await invoke("managed_enroll", {
      account: requireAccount(),
      workspace: workspaceSlug,
      project,
      hostLabel,
      chat,
    });
  } catch (error) {
    throw new Error(cliErrorMessage(error), { cause: error });
  }
  if (!workspaces.has(workspaceSlug)) {
    workspaces.add(workspaceSlug);
    saveEnrollmentCache();
  }
}

/** The bundled engine must be present; the doctor also names this host. */
async function checkBundle() {
  const doctor = await invoke<{
    runner_present: boolean;
    engine_present: boolean;
    host_label: string;
  }>("managed_doctor");
  if (!doctor.runner_present || !doctor.engine_present)
    throw new Error("Pi Dash Agent needs repair. Reinstall the desktop app to restore its bundled engine.");
  if (hostLabel && hostLabel !== doctor.host_label) forgetEnrollments();
  hostLabel = doctor.host_label;
}

export async function connectAgentProject(workspaceSlug: string, projectId: string) {
  if (!isDesktop()) return;
  const current = generation;
  return enqueue(async () => {
    if (stopped || current !== generation) throw new Error("Pi Dash Agent is signed out.");
    await checkBundle();
    await configure();
    if (stopped || current !== generation) return;
    await ensureCliSignedIn(workspaceSlug);
    if (stopped || current !== generation) return;
    const project = await api<{ identifier: string }>(`/api/workspaces/${workspaceSlug}/projects/${projectId}/`);
    // A project the user connected themselves is the one chat should ride on.
    const forChat = (chatProjects.get(workspaceSlug) ?? project.identifier) === project.identifier;
    await enroll(workspaceSlug, project.identifier, forChat);
    if (!chatProjects.has(workspaceSlug)) {
      chatProjects.set(workspaceSlug, project.identifier);
      saveEnrollmentCache();
    }
    if (stopped || current !== generation) return;
    await startDaemon();
  });
}

type ListedProject = { identifier?: string; is_default?: boolean; archived_at?: string | null; created_at?: string };

/**
 * The project a workspace's local chat enrols against, resolved once.
 *
 * The project list comes back in no guaranteed order (the endpoint groups, so
 * the model's default ordering is dropped), which is why position is never
 * used: the workspace default wins, then the oldest project — neither moves
 * when someone creates a new one.
 */
async function resolveChatProject(workspaceSlug: string): Promise<string> {
  const known = chatProjects.get(workspaceSlug);
  if (known) return known;
  const projects = await api<{ results?: ListedProject[] } | ListedProject[]>(
    `/api/workspaces/${workspaceSlug}/projects/`
  );
  const list = (Array.isArray(projects) ? projects : (projects.results ?? [])).filter((project) => project?.identifier);
  const active = list.filter((project) => !project.archived_at);
  const candidates = active.length ? active : list;
  let oldest = candidates[0];
  for (const project of candidates) if ((project.created_at ?? "") < (oldest.created_at ?? "")) oldest = project;
  const identifier = (candidates.find((project) => project.is_default) ?? oldest)?.identifier;
  if (!identifier) {
    throw new Error("Create a project in this workspace before chatting with the built-in agent.");
  }
  chatProjects.set(workspaceSlug, identifier);
  saveEnrollmentCache();
  return identifier;
}

/**
 * Make the bundled runtime usable for a **local chat**, independent of any
 * project the user has opened.
 *
 * Local chat talks to the shared daemon (the daemon owns every engine
 * process), so the daemon has to be up with the engine's runner in it before
 * the first message — otherwise a user who signed in and opened the built-in
 * agent got a chat box with nothing behind it.
 *
 * Same sequence as {@link connectAgentProject}, minus the caller-supplied
 * project: enrolment still registers the engine against one project, so one
 * is picked per workspace and kept (see {@link resolveChatProject}), and that
 * runner is the one chat requests name. It is idempotent and serialised
 * through the same queue, and the full sequence runs once per workspace per
 * page: after that a warm/send only renews an expiring model credential and
 * checks the daemon, so it costs no network. Profile changes are picked up by
 * {@link refreshAgentRuntime}, not here.
 */
/**
 * Whether this server will let the bundled agent run, and why not when it
 * won't. Backs the picker's "Built-in agent" entry so a server with
 * `MANAGED_RUNNER_ENABLED=false` shows the reason up front instead of
 * accepting a message it can never send.
 *
 * Never throws: an unreachable API or an expired session is reported as
 * unavailable with the message, which is exactly what the picker wants to
 * show.
 */
export async function agentAvailability(): Promise<{ available: boolean; reason?: string }> {
  if (!isDesktop()) return { available: false };
  try {
    const profile = await api<Profile>("/api/users/me/ai-assistant/agent-profile/");
    if (profile.available) return { available: true };
    return {
      available: false,
      reason: AGENT_RUNTIME_REASON_MESSAGES[profile.reason_code] ?? profile.reason_code,
    };
  } catch (error) {
    return { available: false, reason: error instanceof Error ? error.message : undefined };
  }
}

export async function ensureChatRuntime(workspaceSlug: string): Promise<void> {
  if (!isDesktop()) return;
  const current = generation;
  return enqueue(async () => {
    if (stopped || current !== generation) throw new Error("Pi Dash Agent is signed out.");
    if (chatReady.get(workspaceSlug) === chatStamp()) {
      if (tokenExpiresAt < Date.now() + 120_000) await configure();
      if (stopped || current !== generation) return;
      // A no-op while the daemon runs; restarts it if it died.
      await startDaemon();
      return;
    }
    await checkBundle();
    // Writes the engine config and a fresh model credential — chat cannot
    // reach a model without it.
    await configure();
    if (stopped || current !== generation) return;
    await ensureCliSignedIn(workspaceSlug);
    if (stopped || current !== generation) return;
    const identifier = await resolveChatProject(workspaceSlug);
    await enroll(workspaceSlug, identifier, true);
    if (stopped || current !== generation) return;
    await startDaemon();
    chatReady.set(workspaceSlug, chatStamp());
  });
}

export async function prepareAgentRun(target: AgentRunTarget): Promise<void> {
  if (!isDesktop()) return;
  const { workspaceSlug, projectId, issueId } = target;
  const projectPath = `/api/workspaces/${workspaceSlug}/projects/${projectId}/`;
  const [issue, initialProject] = await Promise.all([
    api<{ agent_executor?: string | null }>(`${projectPath}issues/${issueId}/`),
    api<{ default_agent_executor?: string }>(projectPath),
  ]);
  // Preparing a run must respect both explicit pins and the project default.
  // Selection is a separate user action; never mutate it from a Run click.
  if ((issue.agent_executor ?? initialProject.default_agent_executor ?? "local_runner") !== "managed_runner") return;
  await connectAgentProject(workspaceSlug, projectId);
  // Enrollment precedes the first heartbeat. Wait for the server to observe
  // the daemon before attempting to create a run.
  for (let attempt = 0; attempt < 30; attempt++) {
    if (stopped) throw new Error("Pi Dash Agent is signed out.");
    // eslint-disable-next-line no-await-in-loop -- Poll until enrollment's first heartbeat arrives.
    const project = await api<{ agent_executor_options: { kind: string; available: boolean }[] }>(projectPath);
    if (project.agent_executor_options?.some((option) => option.kind === "managed_runner" && option.available)) {
      return;
    }
    // eslint-disable-next-line no-await-in-loop -- Delay between heartbeat availability polls.
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error("Pi Dash Agent has not connected yet. Try Run again shortly.");
}

export async function refreshAgentRuntime() {
  if (!isDesktop() || stopped || !workspaces.size) return;
  return enqueue(async () => {
    if (stopped) return;
    try {
      await configure();
    } catch (error) {
      chatReady.clear();
      throw error;
    }
    if (stopped) return;
    // One shared daemon serves every workspace.
    await startDaemon();
  });
}

/**
 * Tear the runtime down for a sign-out.
 *
 * Signing the app out signs this computer's `pidash` CLI out first
 * (`pidash auth logout --yes`, after `confirmSignOut` warned about runs in
 * progress): one sign-in covers both, and a CLI left signed in would hand its
 * session straight back to the app on the next account-sync pass. If the CLI
 * cannot be signed out, the sign-out stops there and says why. Not when the
 * app is following the CLI's own sign-out (see {@link signOutFollowingCli}).
 *
 * `choice` comes from the sign-out dialog and is the only thing that can make
 * a sign-out delete chat history. The session-expiry teardown in `api` and the
 * second cleanup pass below never pass it.
 */
export async function disposeAgentRuntime(choice?: SignOutChoice): Promise<void> {
  if (!isDesktop()) return;
  if (!followingCli) await signDaemonOut();
  stopped = true;
  generation++;
  clearEnrollmentCache();
  // Stop immediately, then clean again after any in-flight enrollment finishes.
  await invoke(
    "managed_sign_out",
    choice?.deleteChatHistory && activeUserId ? { clearChatHistory: { account: activeUserId } } : undefined
  );
  await enqueue(async () => {
    try {
      await invoke("managed_sign_out");
    } finally {
      forgetEnrollments();
      tokenExpiresAt = 0;
    }
  });
  // The app's own sign-out time, for account sync's "most recent wins".
  await observeDesktopAuth(null).catch(() => {});
}
