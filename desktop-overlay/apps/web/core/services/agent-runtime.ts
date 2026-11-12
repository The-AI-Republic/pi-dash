/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The desktop implementation of the `apps/web` agent-runtime seam.
 *
 * The app owns no daemon, runner or engine any more
 * (`.ai_design/built_engine_as_runner/design.md` §4.1): this computer's Pi
 * Dash daemon — the CLI's, run as a user service — hosts them, and the app is
 * its client (`desktop-daemon.ts`). So this module no longer enrols a runner
 * when a project opens, writes engine config or model tokens, or starts and
 * stops a process. What is left:
 *
 * - the signed-in account, which scopes local chat history;
 * - the sign-out confirmation — signing out of the app signs out only the app,
 *   with an opt-in to also run `pidash auth logout` (design §4.6);
 * - "Run on this computer": turning the built-in engine on for a project the
 *   first time it is used (design §5, enrol on use);
 * - making local chat ready: a compatible daemon, and an engine sign-in that
 *   is current (renewed silently only when `expiring` or `expired`).
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@pi-dash/services";
import { AGENT_RUNTIME_REASON_MESSAGES } from "@/pi-dash-web/components/desktop/agent-runtime-edition";
import { restoreApprovalModes } from "@/services/desktop-approval-modes";
import {
  compatibleHello,
  currentDiscovery,
  discoverDaemon,
  discoveryProblem,
  engineActionFor,
  engineSignInState,
  invoke,
  isDesktopHost,
  renewEngineIfAllowed,
  setAppIdentity,
  type EngineRunnerInfo,
  type LocalRunner,
} from "@/services/desktop-daemon";
import { requestDaemonSetup, type SetUpReason } from "@/services/pidash-cli";

class AgentAPI extends APIService {}
const agentAPI = new AgentAPI(API_BASE_URL);

export type AgentRunTarget = { workspaceSlug: string; projectId: string; issueId: string };
/** What the user chose in the sign-out dialog. */
export type SignOutChoice = {
  deleteChatHistory: boolean;
  /** "Also sign out Pi Dash on this computer": `pidash auth logout`. */
  signOutComputer?: boolean;
};
type SignOutPrompt = (resolve: (choice: SignOutChoice | null) => void) => void;

export function isDesktop() {
  return isDesktopHost();
}

let activeUserId = "";
let topUpUrl = "";

/**
 * The signed-in account id, set by {@link resumeAgentRuntime}. Local chat
 * history is scoped per account. Empty until the runtime resumed for a user.
 */
export function getAgentAccount(): string {
  return activeUserId;
}

/** Where the user adds model credit, when the server names a place. */
export function getModelTopUpUrl(): string {
  return topUpUrl;
}

/**
 * Start (or switch) the runtime for the signed-in user. Discovery is
 * evaluated for this account: a daemon signed in as someone else is refused.
 */
export function resumeAgentRuntime(userId?: string, email?: string) {
  if (!userId) return;
  if (userId !== activeUserId) {
    activeUserId = userId;
    topUpUrl = "";
    // A sign-out wiped the webview's storage; put this account's approval
    // mode back before the user can reach a chat.
    void restoreApprovalModes(userId).catch(() => {});
  }
  setAppIdentity({ userId, email, cloudUrl: API_BASE_URL });
}

/**
 * Re-check the daemon and keep the engine signed in: called on launch, on
 * focus and every minute. Silent renewal happens only in the two states that
 * allow it (`expiring`, `expired`). Never throws.
 */
export async function refreshAgentRuntime(): Promise<void> {
  if (!isDesktop() || !activeUserId) return;
  await discoverDaemon().catch(() => null);
  await renewEngineIfAllowed();
  void refreshTopUpUrl();
}

async function refreshTopUpUrl() {
  // Best-effort: only for the "add credit" link in the chat transcript.
  try {
    const response: { data?: { top_up_url?: string } } = await agentAPI.request({
      url: "/api/users/me/ai-assistant/agent-profile/",
      method: "GET",
    });
    topUpUrl = response.data?.top_up_url ?? "";
  } catch {
    // Keep what we had.
  }
}

/**
 * Whether the built-in engine on this computer can take a chat now, and why
 * not. Backs the "This computer" contact. Never throws.
 */
export async function agentAvailability(): Promise<{ available: boolean; reason?: string }> {
  if (!isDesktop()) return { available: false };
  const discovery = (await discoverDaemon().catch(() => null)) ?? currentDiscovery();
  const hello = compatibleHello(discovery);
  if (!hello) {
    // No daemon yet is not a dead end: opening the chat offers set-up.
    if (discovery?.kind === "no_daemon" || discovery?.kind === "signed_out") return { available: true };
    return { available: false, reason: discoveryProblem(discovery) ?? undefined };
  }
  const engine = hello.engine_detail;
  if (hello.engine === "unavailable") {
    const code = engine?.reason_code ?? "";
    return {
      available: false,
      reason: AGENT_RUNTIME_REASON_MESSAGES[code] ?? (code || "The built-in engine is unavailable."),
    };
  }
  if (engineActionFor(engineSignInState(hello)) === "sign_in")
    return { available: false, reason: "The built-in engine is signed out. Sign it in from Built-in engine settings." };
  return { available: true };
}

/**
 * Make the daemon ready for a local chat: set it up if there is none (asking
 * first), renew the engine credential when that is allowed, and refuse with
 * the reason otherwise.
 */
export async function ensureChatRuntime(workspaceSlug: string): Promise<void> {
  if (!isDesktop()) return;
  await ensureDaemon("chat", workspaceSlug);
  await renewEngineIfAllowed();
  const hello = compatibleHello();
  const action = engineActionFor(engineSignInState(hello));
  if (action === "sign_in" || action === "turn_on") {
    throw new Error(
      action === "turn_on"
        ? "Turn on the built-in engine in Built-in engine settings to chat with it."
        : "The built-in engine is signed out. Sign it in from Built-in engine settings."
    );
  }
}

/**
 * A daemon the app can use, set up on the user's say-so when there is none.
 * Throws the reason when there is still none.
 */
export async function ensureDaemon(reason: SetUpReason, workspaceSlug: string): Promise<void> {
  if (compatibleHello((await discoverDaemon().catch(() => null)) ?? currentDiscovery())) return;
  if (await requestDaemonSetup(reason, workspaceSlug)) return;
  throw new Error(discoveryProblem(currentDiscovery()) ?? "Pi Dash is not set up on this computer.");
}

/**
 * The model gateway refused the engine's credential mid-chat. The daemon owns
 * model tokens now, so the app can only renew the engine sign-in — silently,
 * when its state allows. Rejects when it cannot; the caller asks the user to
 * sign in.
 */
export async function refreshModelCredential(): Promise<void> {
  if (!isDesktop()) return;
  await discoverDaemon().catch(() => null);
  if (!(await renewEngineIfAllowed())) throw new Error("The built-in engine needs a new sign-in.");
}

/** The engine runner this computer has for a project, if any. */
export async function localEngineRunner(workspaceSlug: string, project: string): Promise<LocalRunner | undefined> {
  const runners = await invoke<LocalRunner[]>("daemon_runners");
  return runners.find((r) => r.agent === "engine" && r.workspace_slug === workspaceSlug && r.project_slug === project);
}

/**
 * Turn the built-in engine on for a project on this computer, unless it is
 * already (design §5). Sets the daemon up first when there is none.
 */
export async function turnOnEngineForProject(
  workspaceSlug: string,
  project: string,
  workingDir?: string
): Promise<EngineRunnerInfo | LocalRunner> {
  await ensureDaemon("engine", workspaceSlug);
  const existing = await localEngineRunner(workspaceSlug, project);
  if (existing) return existing;
  return invoke<EngineRunnerInfo>("daemon_engine_runner_add", {
    workspace: workspaceSlug,
    project,
    workingDir: workingDir || null,
  });
}

async function api<T>(path: string): Promise<T> {
  const response: { data: T } = await agentAPI.request({ url: path, method: "GET" });
  return response.data;
}

/**
 * Before a Run on an issue set to the built-in engine (the legacy
 * `managed_runner` executor the server still translates): turn the engine on
 * for that project on this computer — the first Run is what enrols it — and
 * wait for the server to see it online. Any other executor is left alone; the
 * Run click never changes the issue's choice.
 */
export async function prepareAgentRun(target: AgentRunTarget): Promise<void> {
  if (!isDesktop()) return;
  const { workspaceSlug, projectId, issueId } = target;
  const projectPath = `/api/workspaces/${workspaceSlug}/projects/${projectId}/`;
  const [issue, project] = await Promise.all([
    api<{ agent_executor?: string | null }>(`${projectPath}issues/${issueId}/`),
    api<{ identifier: string; default_agent_executor?: string }>(projectPath),
  ]);
  if ((issue.agent_executor ?? project.default_agent_executor ?? "local_runner") !== "managed_runner") return;
  await turnOnEngineForProject(workspaceSlug, project.identifier);
  for (let attempt = 0; attempt < 30; attempt++) {
    // eslint-disable-next-line no-await-in-loop -- poll until the engine's first heartbeat arrives
    const current = await api<{ agent_executor_options?: { kind: string; available: boolean }[] }>(projectPath);
    if (current.agent_executor_options?.some((o) => o.kind === "managed_runner" && o.available)) return;
    // eslint-disable-next-line no-await-in-loop -- delay between availability polls
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error("The built-in engine on this computer has not connected yet. Try Run again shortly.");
}

let signOutPrompt: SignOutPrompt | null = null;

/** Mounts (or, with `null`, unmounts) the dialog {@link confirmSignOut} opens. */
export function registerSignOutPrompt(prompt: SignOutPrompt | null) {
  signOutPrompt = prompt;
}

/**
 * Ask the user to confirm a sign-out they started. Resolves `null` when they
 * cancel. Both options are opt-in: with no dialog to ask through, sign-out
 * proceeds, keeps the history and leaves Pi Dash on this computer signed in.
 */
export function confirmSignOut(): Promise<SignOutChoice | null> {
  const prompt = signOutPrompt;
  if (!isDesktop() || !prompt) return Promise.resolve({ deleteChatHistory: false });
  return new Promise((resolve) => prompt(resolve));
}

/** Whether the sign-out dialog should offer to sign out the computer too. */
export function canSignOutComputer(): boolean {
  return compatibleHello() !== null;
}

/**
 * The app half of a sign-out. The daemon, the engine and every runner stay
 * signed in (they are independent of the app) unless the user ticked "Also
 * sign out Pi Dash on this computer". Chat history goes only when asked for.
 */
export async function disposeAgentRuntime(choice?: SignOutChoice): Promise<void> {
  if (!isDesktop()) return;
  const account = activeUserId;
  if (choice?.deleteChatHistory && account) {
    await invoke("chat_clear_history", { account }).catch(() => {
      console.warn("Deleting local chat history failed; continuing sign-out.");
    });
  }
  if (choice?.signOutComputer) {
    await invoke("pidash_cli_logout", { engine: false }).catch(() => {
      console.warn("Signing out Pi Dash on this computer failed; continuing sign-out.");
    });
  }
  // The account stays known until the page navigates away: the session
  // teardown after this still stashes its approval mode under it.
  setAppIdentity(null);
}
