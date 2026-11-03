// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Local agent-runtime service (RUN-044, RUN-045, RUN-047). Owns the
// desktop-only lifecycle the supervisor component drives: resuming the
// runtime for the signed account, connecting projects (self-check,
// engine setup, credential, enrollment, daemon start), the background
// refresh, the chat/run preparation the desktop-chat and dispatch slices
// consume, and the two-phase sign-out disposal. Every entry takes the API
// client plus the platform capability and is inert without the capability,
// so the web and cloud builds run no step and make no call.

import {
  createDesktopEnroll,
  deleteDesktopEnroll,
  getAgentProfile,
  getIssue,
  getCloudCsrfToken,
  getProject,
  isApiError,
  listProjects,
  postAgentToken,
  type AgentToken,
  type ApiClient,
} from "@pidash/api-client";

import { edition } from "../../../core/edition/index.js";
import type { AgentHostCheck, AgentRuntimeApi } from "../../../core/platform/index.js";
import type { RuntimeSignOutChoice } from "../../../core/session/index.js";
import { agentReasonMessage } from "../../../shared/lib/agentReasons.js";
import { restoreApprovalModes, stashApprovalModes } from "./approvalModes.js";
import { agentRefusalCode, isKnownAgentReason } from "./reasons.js";

/** Server executor kind for the bundled on-machine agent. */
export const MANAGED_RUNNER_KIND = "managed_runner";

/** Renew the model credential when less than this much life remains. */
export const CREDENTIAL_RENEW_BEFORE_MS = 120_000;

/** Heartbeat wait after preparing a run: attempts and pause between them. */
export const HEARTBEAT_POLL_ATTEMPTS = 30;
export const HEARTBEAT_POLL_INTERVAL_MS = 1000;

/** The server refuses to run the bundled agent; carries its reason code. */
export class EngineUnavailableError extends Error {
  readonly code: string;

  constructor(code: string) {
    super(agentReasonMessage(code));
    this.name = "EngineUnavailableError";
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

interface RuntimeState {
  stopped: boolean;
  generation: number;
  activeUserId: string;
  hostLabel: string;
  tokenExpiresAt: number;
  workspaces: Set<string>;
  chatProjects: Map<string, string>;
  chatReady: Map<string, string>;
  pending: Promise<unknown>;
}

const state: RuntimeState = {
  stopped: false,
  generation: 0,
  activeUserId: "",
  hostLabel: "",
  tokenExpiresAt: 0,
  workspaces: new Set(),
  chatProjects: new Map(),
  chatReady: new Map(),
  pending: Promise.resolve(),
};

/** Drop all runtime state. Tests only; the app resumes instead. */
export function resetRuntimeServiceForTests(): void {
  state.stopped = false;
  state.generation = 0;
  state.activeUserId = "";
  state.hostLabel = "";
  state.tokenExpiresAt = 0;
  state.workspaces.clear();
  state.chatProjects.clear();
  state.chatReady.clear();
  state.pending = Promise.resolve();
}

/**
 * The signed-in account id, set by resumeRuntime. Local chat history is
 * scoped per account, so the chat transport keys its store by this value.
 * Empty until the runtime has resumed for a user (always on web).
 */
export function getRuntimeAccount(): string {
  return state.activeUserId;
}

/** The account every engine command is scoped to. */
function requireAccount(): string {
  if (!state.activeUserId) throw new Error("The local agent is signed out.");
  return state.activeUserId;
}

/** True when a queued action must not run (signed out, or superseded by a newer sign-in). */
function isCancelled(generation: number): boolean {
  return state.stopped || generation !== state.generation;
}

/** Serialize config mutations; a project-open effect and a Run click can overlap. */
function enqueue<T>(action: () => Promise<T>): Promise<T> {
  const next = state.pending.then(action, action);
  state.pending = next.catch(() => undefined);
  return next;
}

function enrollmentCacheKey(): string {
  return `pidash-managed-workspaces:${state.activeUserId}`;
}

function saveEnrollmentCache(): void {
  // The account is always set here: saving only happens after configure,
  // which scopes every engine command to the resumed account.
  try {
    globalThis.sessionStorage.setItem(
      enrollmentCacheKey(),
      JSON.stringify({
        hostLabel: state.hostLabel,
        workspaces: [...state.workspaces],
        chatProjects: Object.fromEntries(state.chatProjects),
      })
    );
  } catch {
    // Storage can be unavailable; enrollment still works for this page.
  }
}

function clearEnrollmentCache(): void {
  if (!state.activeUserId) return;
  try {
    globalThis.sessionStorage.removeItem(enrollmentCacheKey());
  } catch {
    // Native sign-out still deletes the credentials.
  }
}

function forgetEnrollments(): void {
  state.workspaces.clear();
  state.chatProjects.clear();
  clearEnrollmentCache();
}

interface CachedEnrollments {
  hostLabel?: unknown;
  workspaces?: unknown;
  chatProjects?: unknown;
}

function loadEnrollmentCache(): void {
  let cached: CachedEnrollments | null = null;
  try {
    cached = JSON.parse(globalThis.sessionStorage.getItem(enrollmentCacheKey()) ?? "null") as CachedEnrollments | null;
  } catch {
    clearEnrollmentCache();
    return;
  }
  if (typeof cached?.hostLabel === "string" && Array.isArray(cached.workspaces)) {
    state.hostLabel = cached.hostLabel;
    for (const workspace of cached.workspaces) {
      if (typeof workspace === "string") state.workspaces.add(workspace);
    }
    if (cached.chatProjects !== null && typeof cached.chatProjects === "object") {
      for (const [workspace, project] of Object.entries(cached.chatProjects)) {
        if (typeof project === "string") state.chatProjects.set(workspace, project);
      }
    }
  }
}

/**
 * Run an API call, tearing the local runtime down on a 401: stop, forget
 * enrollments, drop the credential and sign the native side out. Anything
 * else propagates untouched; the original 401 propagates too, so callers
 * still see the real failure (the session middleware turns it into the
 * expired-session landing — nothing here forges a logout).
 */
async function withSessionRecovery<T>(runtime: AgentRuntimeApi, action: () => Promise<T>): Promise<T> {
  try {
    return await action();
  } catch (error) {
    if (!isApiError(error) || error.status !== 401) throw error;
    state.stopped = true;
    state.generation += 1;
    state.tokenExpiresAt = 0;
    forgetEnrollments();
    try {
      await runtime.signOutNative();
    } catch {
      // The caller still needs the original failure below.
    }
    throw error;
  }
}

function assertHostUsable(doctor: AgentHostCheck): void {
  if (!doctor.runnerPresent || !doctor.enginePresent) {
    throw new Error("The desktop app's bundled agent is damaged. Reinstall the app to repair it.");
  }
}

/**
 * Mint the engine credential through the build's CSRF path: the cloud
 * deployment proxies only `/api/*`, so the agent reads its token from the
 * cloud sibling instead of the client's default `/auth/` read
 * (CLOUD-040, parity/specs/cloud.md §8). Every other build keeps the
 * client's own plumbing.
 */
export function agentCredentialRequest(client: ApiClient, options: AgentCredentialOptions): Promise<AgentToken> {
  if (!options.cloudCsrf) return postAgentToken(client);
  return getCloudCsrfToken(client).then((csrf) => postAgentToken(client, { csrfToken: csrf.csrf_token }));
}

/** Which CSRF path the credential mint reads for this build. */
export interface AgentCredentialOptions {
  /** Cloud deployments: read `/api/auth/get-csrf-token/` and send it. */
  cloudCsrf: boolean;
}

/** Mint with the edition's CSRF choice (cloud builds read the `/api/` sibling). */
function mintAgentCredential(client: ApiClient): Promise<AgentToken> {
  return agentCredentialRequest(client, { cloudCsrf: edition.flags["cloud-csrf"] === true });
}

/**
 * A managed-runner refusal whose code this build names: the caller gets
 * the situation-specific sentence, not a generic failure. Any other
 * failure keeps its own error.
 */
export function agentRefusalError(error: unknown): unknown {
  if (!isApiError(error)) return error;
  const code = agentRefusalCode(error.body);
  return code !== null && isKnownAgentReason(code) ? new EngineUnavailableError(code) : error;
}

/** Read the server profile, write the engine setup, renew an expiring credential. */
async function configure(input: { client: ApiClient; runtime: AgentRuntimeApi }): Promise<void> {
  const { client, runtime } = input;
  const profile = await withSessionRecovery(runtime, () => getAgentProfile(client)).catch((error) => {
    throw agentRefusalError(error);
  });
  if (!profile.available) {
    state.chatReady.clear();
    await runtime.stopDaemon();
    throw new EngineUnavailableError(profile.reason_code);
  }
  await runtime.writeEngineConfig({
    account: requireAccount(),
    profile: {
      available: profile.available,
      base_url: profile.base_url,
      model: profile.model,
      reason_code: profile.reason_code,
    },
  });
  if (state.tokenExpiresAt < Date.now() + CREDENTIAL_RENEW_BEFORE_MS) {
    const credential = await withSessionRecovery(runtime, () => mintAgentCredential(client)).catch((error) => {
      throw agentRefusalError(error);
    });
    const expiresAt = Date.parse(credential.expires_at);
    if (!credential.token || !Number.isFinite(expiresAt) || expiresAt <= Date.now()) {
      throw new Error("The desktop session expired. Sign in again.");
    }
    await runtime.writeModelToken(credential.token);
    state.tokenExpiresAt = expiresAt;
  }
}

/**
 * The project a workspace's local chat enrolls against, resolved once. The
 * project list comes back in no guaranteed order, which is why position is
 * never used: the workspace default wins, then the oldest project — neither
 * moves when someone creates a new one.
 */
async function resolveChatProject(input: {
  client: ApiClient;
  runtime: AgentRuntimeApi;
  workspaceSlug: string;
}): Promise<string> {
  const { client, runtime, workspaceSlug } = input;
  const known = state.chatProjects.get(workspaceSlug);
  if (known) return known;
  const projects = await withSessionRecovery(runtime, () => listProjects(client, workspaceSlug));
  const live = projects.filter((project) => !project.archived_at);
  const candidates = live.length > 0 ? live : projects;
  const first = candidates[0];
  if (!first) {
    throw new Error("This workspace needs a project before the built-in agent can start.");
  }
  let oldest = first;
  for (const candidate of candidates) {
    if (candidate.created_at < oldest.created_at) oldest = candidate;
  }
  const identifier = candidates.find((project) => project.is_default)?.identifier ?? oldest.identifier;
  state.chatProjects.set(workspaceSlug, identifier);
  saveEnrollmentCache();
  return identifier;
}

/** Enroll this machine against a workspace it has not enrolled in this session. */
async function enrollWorkspace(input: {
  client: ApiClient;
  runtime: AgentRuntimeApi;
  workspaceSlug: string;
}): Promise<void> {
  const { client, runtime, workspaceSlug } = input;
  if (state.workspaces.has(workspaceSlug)) return;
  const enrollment = await withSessionRecovery(runtime, () =>
    createDesktopEnroll(client, { workspaceSlug, hostLabel: state.hostLabel })
  );
  await runtime.bootstrapMachine({
    cloudUrl: client.baseUrl,
    workspace: workspaceSlug,
    machineToken: enrollment.machine_token,
    devMachineId: enrollment.dev_machine_id,
  });
  state.workspaces.add(workspaceSlug);
  saveEnrollmentCache();
}

/** Self-check the host and refresh the cached host label (forgetting enrollments on change). */
async function checkHostAndLabel(runtime: AgentRuntimeApi): Promise<void> {
  const doctor = await runtime.checkHost();
  assertHostUsable(doctor);
  if (state.hostLabel && state.hostLabel !== doctor.hostLabel) forgetEnrollments();
  state.hostLabel = doctor.hostLabel;
}

/**
 * Resume the runtime for the signed account: restore the per-user
 * enrollment cache so a reload reuses the running daemon, and hand back
 * the approval modes stashed at the last sign-out. Inert without the
 * desktop capability.
 */
export function resumeRuntime(userId: string | undefined, deps: RuntimeDeps): void {
  const { runtime } = deps;
  if (!runtime) return;
  if (userId && userId !== state.activeUserId) {
    state.activeUserId = userId;
    state.workspaces.clear();
    state.chatProjects.clear();
    loadEnrollmentCache();
    // A sign-out wiped the webview's storage; put this account's approval
    // modes back before the user can reach a chat.
    void restoreApprovalModes({ account: userId, runtime }).catch(() => undefined);
  }
  state.stopped = false;
}

/**
 * Connect a project: self-check, engine setup and credential, workspace
 * enrollment, project enrollment, daemon start. Inert without the desktop
 * capability.
 */
export async function connectProject(input: { workspaceSlug: string; projectId: string } & RuntimeDeps): Promise<void> {
  const { client, runtime, workspaceSlug, projectId } = input;
  if (!runtime) return;
  const current = state.generation;
  return enqueue(async () => {
    if (isCancelled(current)) throw new Error("The local agent is signed out.");
    await checkHostAndLabel(runtime);
    await configure({ client, runtime });
    if (isCancelled(current)) return;
    await enrollWorkspace({ client, runtime, workspaceSlug });
    const project = await withSessionRecovery(runtime, () => getProject(client, workspaceSlug, projectId));
    await runtime.enrollProject({
      account: requireAccount(),
      workspace: workspaceSlug,
      project: project.identifier,
      hostLabel: state.hostLabel,
    });
    // A project the user connected themselves is the one chat should ride on.
    if (!state.chatProjects.has(workspaceSlug)) {
      state.chatProjects.set(workspaceSlug, project.identifier);
      saveEnrollmentCache();
    }
    if (isCancelled(current)) return;
    await runtime.startDaemon({ account: requireAccount(), workspace: workspaceSlug });
  });
}

function chatStamp(): string {
  return `${state.generation}:${state.activeUserId}:${state.hostLabel}`;
}

/**
 * Make the bundled runtime usable for a local chat, independent of any
 * project the user has opened. Same sequence as connectProject, minus the
 * caller-supplied project: enrollment still registers this machine against
 * one project, so one is picked per workspace and kept. Idempotent — the
 * full sequence runs once per workspace per page; after that a warm or
 * send only renews an expiring credential and checks the daemon.
 */
export async function ensureChatRuntime(input: { workspaceSlug: string } & RuntimeDeps): Promise<void> {
  const { client, runtime, workspaceSlug } = input;
  if (!runtime) return;
  const current = state.generation;
  return enqueue(async () => {
    if (isCancelled(current)) throw new Error("The local agent is signed out.");
    if (state.chatReady.get(workspaceSlug) === chatStamp()) {
      if (state.tokenExpiresAt < Date.now() + CREDENTIAL_RENEW_BEFORE_MS) {
        await configure({ client, runtime });
      }
      if (isCancelled(current)) return;
      // A no-op while the daemon runs; restarts it if it died.
      await runtime.startDaemon({ account: requireAccount(), workspace: workspaceSlug });
      return;
    }
    await checkHostAndLabel(runtime);
    await configure({ client, runtime });
    if (isCancelled(current)) return;
    await enrollWorkspace({ client, runtime, workspaceSlug });
    const identifier = await resolveChatProject({ client, runtime, workspaceSlug });
    await runtime.enrollProject({
      account: requireAccount(),
      workspace: workspaceSlug,
      project: identifier,
      hostLabel: state.hostLabel,
    });
    if (isCancelled(current)) return;
    await runtime.startDaemon({ account: requireAccount(), workspace: workspaceSlug });
    state.chatReady.set(workspaceSlug, chatStamp());
  });
}

/**
 * Prepare a user-triggered agent run. Only when the work item actually
 * targets this machine (explicit pin, else the project default): then run
 * the project connection and wait briefly for the server to observe the
 * daemon before the run is created. Selection is never mutated here.
 */
export async function prepareAgentRun(
  input: { target: AgentRunTarget; heartbeatPoll?: HeartbeatPoll | undefined } & RuntimeDeps
): Promise<void> {
  const { client, runtime, target } = input;
  if (!runtime) return;
  const { workspaceSlug, projectId, issueId } = target;
  const [issue, initialProject] = await Promise.all([
    withSessionRecovery(runtime, () => getIssue(client, workspaceSlug, projectId, issueId)),
    withSessionRecovery(runtime, () => getProject(client, workspaceSlug, projectId)),
  ]);
  if ((issue.agent_executor ?? initialProject.default_agent_executor) !== MANAGED_RUNNER_KIND) {
    return;
  }
  await connectProject({ client, runtime, workspaceSlug, projectId });
  const attempts = input.heartbeatPoll?.maxAttempts ?? HEARTBEAT_POLL_ATTEMPTS;
  const intervalMs = input.heartbeatPoll?.intervalMs ?? HEARTBEAT_POLL_INTERVAL_MS;
  const sleep = input.heartbeatPoll?.sleep ?? defaultSleep;
  for (let attempt = 0; attempt < attempts; attempt++) {
    if (state.stopped) throw new Error("The local agent is signed out.");
    const project = await withSessionRecovery(runtime, () => getProject(client, workspaceSlug, projectId));
    if (project.agent_executor_options.some((option) => option.kind === MANAGED_RUNNER_KIND && option.available)) {
      return;
    }
    await sleep(intervalMs);
  }
  throw new Error("The local agent is still connecting. Try once more shortly.");
}

function defaultSleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Background refresh: re-read the profile, rewrite the engine setup,
 * rotate a near-expiry credential, restart each workspace daemon. An
 * expired credential stops the daemon instead. Nothing to do without
 * enrolled workspaces. Failures propagate to the caller (the supervisor
 * surfaces them); the ready map is dropped so the next chat re-runs the
 * full sequence.
 */
export async function refreshRuntime(deps: RuntimeDeps): Promise<void> {
  const { client, runtime } = deps;
  if (!runtime || state.stopped || state.workspaces.size === 0) return;
  return enqueue(async () => {
    if (state.stopped) return;
    try {
      await configure({ client, runtime });
    } catch (error) {
      state.chatReady.clear();
      if (state.tokenExpiresAt <= Date.now()) await runtime.stopDaemon();
      throw error;
    }
    if (state.stopped) return;
    const account = requireAccount();
    for (const workspace of state.workspaces) {
      await runtime.startDaemon({ account, workspace });
    }
  });
}

/**
 * Tear the runtime down for a sign-out. The choice comes from the sign-out
 * dialog and is the only thing that can make a sign-out delete chat
 * history. Stop immediately, then clean again after any in-flight
 * enrollment finishes (a two-phase stop avoids racing an enrollment still
 * completing). Server enrollment removal is best-effort: an expired
 * session or unreachable server must not trap the user. Never rejects.
 */
export async function disposeRuntimeForSignOut(input: { choice: RuntimeSignOutChoice } & RuntimeDeps): Promise<void> {
  const { client, runtime, choice } = input;
  if (!runtime) return;
  state.stopped = true;
  state.generation += 1;
  clearEnrollmentCache();
  try {
    await stashApprovalModes({ account: state.activeUserId, runtime });
  } catch {
    // Signing out matters more than keeping a preference; wipe regardless.
  }
  try {
    await runtime.signOutNative(
      choice.deleteChatHistory && state.activeUserId ? { deleteChatHistoryAccount: state.activeUserId } : undefined
    );
  } catch {
    // The second pass below retries the native teardown.
  }
  await enqueue(async () => {
    try {
      if (state.hostLabel) await deleteDesktopEnroll(client, state.hostLabel);
    } catch {
      // An expired desktop session or unreachable server must not trap the
      // user in the app. Local credentials are removed below.
    }
    try {
      await runtime.signOutNative();
    } catch {
      // Local credentials are already gone or never existed.
    }
    forgetEnrollments();
    state.tokenExpiresAt = 0;
  });
}
