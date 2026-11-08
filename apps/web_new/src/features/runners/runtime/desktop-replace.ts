// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// "Is this <label>, last seen <date>? Replace it" — recovering after the
// daemon's data was wiped (`.ai_design/built_engine_as_runner/design.md`
// §4.10).
//
// A wiped daemon signs in as a new machine. Two computers can share a
// hostname, so the server never guesses that the new machine replaces an old
// one; it only lists the candidates — the user's offline machines with the
// same `host_label` and no live session. The app asks about the most recently
// seen one it has not been told to leave alone:
//
// - **Replace it** merges the candidate into this machine (its runner rows,
//   pins and pods move, with the collision rule of §13), then has the daemon
//   re-create the local blocks of the moved engine runners — an engine runner
//   needs nothing local but its folder, which the daemon allocates (§10).
//   The daemon's engine reconcile would re-add them too, but only on its next
//   tick (every 10 minutes); the app does it at once, and treats a block the
//   reconcile added meanwhile as restored. Own-agent runners cannot be rebuilt (their agent and folder were local);
//   they stay offline, for "Set up again on this computer" or removal.
// - **Keep it** leaves the old machine offline for the user to remove (PO-9)
//   and is remembered, so the app does not ask about it again.
//
// Legacy desktop machines (`provisioning = desktop_bundled`) are not offered:
// the migration (`desktop-migration`) merges those.

import {
  listReplaceCandidates,
  listRunners,
  listWorkspaces,
  mergeDevMachineInto,
  type ApiClient,
  type MergeResult,
  type ReplaceCandidate,
} from "@pidash/api-client";

import type { AgentRuntimeApi, CliSetupApi, DaemonDiscovery, DaemonLocalRunner } from "../../../core/platform/index.js";
import { dismissNotices, loadNotices, replaceKey } from "./desktop-notices.js";

export type { ReplaceCandidate };

/** The seams the flow touches, injectable for tests. */
export interface ReplaceDeps {
  runtime: AgentRuntimeApi;
  client: ApiClient;
  cli?: CliSetupApi | undefined;
  cloudUrl: string;
  discovery: () => DaemonDiscovery | null;
  signInWorkspace: (workspaceSlug: string) => Promise<void>;
}

export interface ReplacePrompt {
  /** This computer's machine, which the candidate would be merged into. */
  machineId: string;
  candidate: ReplaceCandidate;
}

/**
 * The machine to ask about, or `null`: none, the daemon is not the app's, or
 * every candidate was already declined. Never throws.
 */
export async function findReplacePrompt(account: string, deps: ReplaceDeps): Promise<ReplacePrompt | null> {
  return lookupReplacePrompt(account, deps).catch(() => null);
}

/** {@link findReplacePrompt}, but a failed lookup (offline) rejects. */
async function lookupReplacePrompt(account: string, deps: ReplaceDeps): Promise<ReplacePrompt | null> {
  const discovery = deps.discovery();
  const machineId = discovery?.kind === "compatible" ? discovery.hello.machine_id : undefined;
  if (!machineId) return null;
  const { candidates } = await listReplaceCandidates(deps.client, machineId);
  const { dismissed } = await loadNotices(account, deps.runtime);
  // The server orders them most recently seen first.
  const candidate = (candidates ?? []).find(
    (c) => c.id !== machineId && c.provisioning !== "desktop_bundled" && !dismissed.includes(replaceKey(c.id))
  );
  return candidate ? { machineId, candidate } : null;
}

export interface ReplaceOutcome {
  merge: MergeResult;
  /** Projects whose engine runner the daemon now runs again ("<workspace>/<project>"). */
  restored: string[];
  /** Engine runners the daemon could not re-create, with why. */
  failed: { project: string; error: string }[];
  /** Moved runners that were not engines: offline until set up again. */
  ownAgentRunners: number;
}

/** The merge endpoint's error codes, in the user's words. */
const MERGE_ERRORS: Record<string, string> = {
  source_machine_has_live_session:
    "That computer is still connected to Pi Dash. Quit Pi Dash there, or sign it out, then try again.",
  already_merged_into_another_machine: "That computer was already replaced by another one.",
  target_machine_not_live: "Pi Dash on this computer is not connected right now. Try again in a moment.",
  source_machine_revoked: "That computer was removed from Pi Dash, so there is nothing to replace.",
  dev_machine_not_found: "That computer is no longer in Pi Dash.",
  cannot_merge_into_self: "This is already that computer.",
  dev_machine_mismatch: "That computer belongs to another account.",
};

/**
 * Plain text for an error from the server or the daemon. An api-client
 * `ApiError` carries the server's `{error}` code in its `body`; known codes
 * are mapped to plain text instead of "Request failed with status code 409".
 */
export function errorText(error: unknown): string {
  const body = (error as { body?: unknown } | null)?.body;
  const data =
    (body !== null && typeof body === "object" ? body : null) ??
    (error instanceof Error ? null : (error as { error?: string; detail?: string } | null));
  const record = data as { error?: string; detail?: string } | null;
  const code = record?.error;
  if (code && MERGE_ERRORS[code]) return MERGE_ERRORS[code];
  if (code || record?.detail) return (record?.detail ?? code) as string;
  if (error instanceof Error) return error.message;
  return String(error);
}

interface MovedEngine {
  workspace: string;
  project: string;
}

/** Which moved runners are engines, and in which workspace and project. */
async function movedEngines(moved: string[], deps: ReplaceDeps): Promise<MovedEngine[]> {
  if (!moved.length) return [];
  const wanted = new Set(moved);
  const workspaces = await listWorkspaces(deps.client);
  const engines: MovedEngine[] = [];
  for (const workspace of workspaces ?? []) {
    if (!wanted.size) break;
    // eslint-disable-next-line no-await-in-loop -- stops as soon as every moved runner is found
    const runners = await listRunners(deps.client, workspace.id, { includeBundled: true });
    for (const runner of runners ?? []) {
      if (!wanted.delete(runner.id)) continue;
      const project = runner.pod_detail?.project_identifier;
      if (project) {
        // `is_engine` is not on the web_new Runner contract; built-in
        // engines are the `desktop_bundled` runners. Own-agent runners are
        // the rest (mirrors the legacy `is_engine` gate via provisioning).
        if (runner.provisioning === "desktop_bundled") engines.push({ workspace: workspace.slug, project });
      }
    }
  }
  return engines;
}

/** Whether the daemon has an engine block for `workspace`/`project` (case-insensitive, as the daemon matches). */
function servedIn(local: DaemonLocalRunner[], workspace: string, project: string): boolean {
  return local.some(
    (r) =>
      r.agent === "engine" && r.workspace_slug === workspace && r.project_slug?.toLowerCase() === project.toLowerCase()
  );
}

/**
 * "Replace it": merge `prompt.candidate` into this machine, then re-create the
 * moved engine runners' local blocks on the daemon. The merge is idempotent
 * for the same target, so a retry after a partial failure is safe.
 */
export async function replaceMachine(prompt: ReplacePrompt, deps: ReplaceDeps): Promise<ReplaceOutcome> {
  const discovery = deps.discovery();
  const hello = discovery?.kind === "compatible" ? discovery.hello : null;
  if (hello?.machine_id !== prompt.machineId) {
    throw new Error("Pi Dash on this computer changed meanwhile. Try again.");
  }
  const merge = await mergeDevMachineInto(deps.client, prompt.candidate.id, prompt.machineId);
  const moved = merge.moved_runner_ids ?? [];
  const engines = await movedEngines(moved, deps);
  const restored: string[] = [];
  const failed: { project: string; error: string }[] = [];
  let signedIn = new Set(hello.workspaces.filter((w) => w.signed_in).map((w) => w.slug));
  const local = await deps.runtime.localRunners();
  for (const { workspace, project } of engines) {
    const name = `${workspace}/${project}`;
    try {
      if (!signedIn.has(workspace)) {
        // eslint-disable-next-line no-await-in-loop -- the daemon's config lock serialises these anyway
        await deps.signInWorkspace(workspace);
        const now = deps.discovery();
        const nowHello = now?.kind === "compatible" ? now.hello : null;
        signedIn = new Set([
          ...signedIn,
          ...(nowHello?.workspaces.filter((w) => w.signed_in).map((w) => w.slug) ?? []),
        ]);
      }
      if (!servedIn(local, workspace, project)) {
        // No working dir: the daemon allocates the folder (§10); the server's
        // reuse lookup returns the moved row, so the runner keeps its id.
        // eslint-disable-next-line no-await-in-loop -- one runner at a time
        await deps.runtime.addEngineRunner({ workspace, project });
      }
      restored.push(name);
    } catch (error) {
      // The daemon's own reconcile may have re-added it meanwhile.
      // eslint-disable-next-line no-await-in-loop -- only on a failure
      const now = await deps.runtime.localRunners().catch(() => [] as DaemonLocalRunner[]);
      if (servedIn(now, workspace, project)) restored.push(name);
      else failed.push({ project: name, error: errorText(error) });
    }
  }
  return { merge, restored, failed, ownAgentRunners: Math.max(0, moved.length - engines.length) };
}

/** "Keep it": leave the old machine, and never ask about it again. */
export async function declineReplace(
  account: string,
  prompt: ReplacePrompt,
  deps: Pick<ReplaceDeps, "runtime">
): Promise<void> {
  await dismissNotices(account, [replaceKey(prompt.candidate.id)], deps.runtime);
}

// --- The prompt's mount point -----------------------------------------------

type PromptListener = (prompt: { account: string; prompt: ReplacePrompt } | null) => void;
let promptListener: PromptListener | null = null;

/** Whether the "Replace it" dialog is showing, so other one-time dialogs
 * (the migration notices) wait instead of stacking on top of it. */
let promptOpen = false;
const openListeners = new Set<() => void>();

export function setReplacePromptOpen(open: boolean): void {
  if (promptOpen === open) return;
  promptOpen = open;
  openListeners.forEach((listener) => listener());
}

export function replacePromptOpen(): boolean {
  return promptOpen;
}

export function subscribeReplacePromptOpen(listener: () => void): () => void {
  openListeners.add(listener);
  return () => {
    openListeners.delete(listener);
  };
}

/** The dialog registers here; `null` unregisters. */
export function registerReplacePrompt(listener: PromptListener | null): void {
  promptListener = listener;
}

/**
 * Find a candidate and, when there is one, show the prompt. Resolves whether
 * it did; rejects when the lookup failed (offline), so the caller asks again.
 */
export async function offerReplace(account: string, deps: ReplaceDeps): Promise<boolean> {
  const prompt = await lookupReplacePrompt(account, deps);
  if (!prompt || !promptListener) return false;
  promptListener({ account, prompt });
  return true;
}
