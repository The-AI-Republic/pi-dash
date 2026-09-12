/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@pi-dash/services";
import { AGENT_RUNTIME_REASON_MESSAGES, CSRF_TOKEN_PATH } from "@/pi-dash-web/components/desktop/agent-runtime-edition";

// Shares the SPA's axios setup — including any edition 401 -> refresh ->
// retry interceptor — so a normal access-cookie expiry does not revoke the
// agent where the edition can refresh it.
class AgentAPI extends APIService {}
const agentAPI = new AgentAPI(API_BASE_URL);

export type AgentRunTarget = { workspaceSlug: string; projectId: string; issueId: string };
type Profile = { available: boolean; reason_code: string; base_url: string; model: string };
type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

export function isDesktop() {
  return typeof window !== "undefined" && "__TAURI__" in window;
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
      workspaces.clear();
      clearEnrollmentCache();
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
const workspaces = new Set<string>();
let tokenExpiresAt = 0;
let hostLabel = "";
let activeUserId = "";

function enrollmentKey() {
  return `pidash-managed-workspaces:${activeUserId}`;
}

function saveEnrollmentCache() {
  if (!activeUserId) return;
  try {
    sessionStorage.setItem(enrollmentKey(), JSON.stringify({ hostLabel, workspaces: [...workspaces] }));
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

function enqueue<T>(action: () => Promise<T>): Promise<T> {
  const next = pending.then(action, action);
  pending = next.catch(() => {});
  return next;
}

export function resumeAgentRuntime(userId?: string) {
  if (userId && userId !== activeUserId) {
    activeUserId = userId;
    workspaces.clear();
    try {
      const cached = JSON.parse(sessionStorage.getItem(enrollmentKey()) ?? "null");
      if (typeof cached?.hostLabel === "string" && Array.isArray(cached.workspaces)) {
        hostLabel = cached.hostLabel;
        for (const workspace of cached.workspaces) if (typeof workspace === "string") workspaces.add(workspace);
      }
    } catch {
      clearEnrollmentCache();
    }
  }
  stopped = false;
}

async function configure() {
  const profile = await api<Profile>("/api/users/me/ai-assistant/agent-profile/");
  if (!profile.available) {
    await invoke("managed_stop_daemon", { graceSeconds: 5 });
    throw new Error(AGENT_RUNTIME_REASON_MESSAGES[profile.reason_code] ?? profile.reason_code);
  }
  await invoke("managed_write_engine_config", { profile });
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

export async function connectAgentProject(workspaceSlug: string, projectId: string) {
  if (!isDesktop()) return;
  const current = generation;
  return enqueue(async () => {
    if (stopped || current !== generation) throw new Error("Pi Dash Agent is signed out.");
    const doctor = await invoke<{
      runner_present: boolean;
      engine_present: boolean;
      host_label: string;
    }>("managed_doctor");
    if (!doctor.runner_present || !doctor.engine_present)
      throw new Error("Pi Dash Agent needs repair. Reinstall the desktop app to restore its bundled engine.");
    if (hostLabel && hostLabel !== doctor.host_label) {
      workspaces.clear();
      clearEnrollmentCache();
    }
    hostLabel = doctor.host_label;
    await configure();
    if (stopped || current !== generation) return;
    if (!workspaces.has(workspaceSlug)) {
      const enrollment = await api<{ machine_token: string; dev_machine_id: string }>(
        "/api/v1/runner/dev-machines/desktop-enroll/",
        "POST",
        { workspace_slug: workspaceSlug, host_label: hostLabel }
      );
      await invoke("managed_bootstrap", {
        cloudUrl: API_BASE_URL,
        workspace: workspaceSlug,
        machineToken: enrollment.machine_token,
        devMachineId: enrollment.dev_machine_id,
      });
      workspaces.add(workspaceSlug);
      saveEnrollmentCache();
    }
    const project = await api<{ identifier: string }>(`/api/workspaces/${workspaceSlug}/projects/${projectId}/`);
    await invoke("managed_enroll", {
      workspace: workspaceSlug,
      project: project.identifier,
      hostLabel,
    });
    if (stopped || current !== generation) return;
    await invoke("managed_start_daemon", { workspace: workspaceSlug });
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
      if (tokenExpiresAt <= Date.now()) await invoke("managed_stop_daemon", { graceSeconds: 5 });
      throw error;
    }
    // eslint-disable-next-line no-await-in-loop -- Serialize daemon lifecycle commands.
    if (!stopped) for (const workspace of workspaces) await invoke("managed_start_daemon", { workspace });
  });
}

export async function disposeAgentRuntime(): Promise<void> {
  if (!isDesktop()) return;
  stopped = true;
  generation++;
  clearEnrollmentCache();
  // Stop immediately, then clean again after any in-flight enrollment finishes.
  await invoke("managed_sign_out");
  await enqueue(async () => {
    try {
      if (hostLabel)
        await api("/api/v1/runner/dev-machines/desktop-enroll/", "DELETE", {
          host_label: hostLabel,
        });
    } catch {
      // An expired desktop session or unreachable server must not trap the
      // user in the app. Local credentials are removed below; the caller
      // must still clear the web session through the regular logout endpoint.
      console.warn("Pi Dash Agent server enrollment cleanup failed; continuing local sign-out.");
    } finally {
      await invoke("managed_sign_out");
      workspaces.clear();
      clearEnrollmentCache();
      tokenExpiresAt = 0;
    }
  });
}
