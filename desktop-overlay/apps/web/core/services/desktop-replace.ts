/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * "Is this <label>, last seen <date>? Replace it" — recovering after the
 * daemon's data was wiped (`.ai_design/built_engine_as_runner/design.md`
 * §4.10).
 *
 * A wiped daemon signs in as a new machine. Two computers can share a
 * hostname, so the server never guesses that the new machine replaces an old
 * one; it only lists the candidates — the user's offline machines with the
 * same `host_label` and no live session. The app asks about the most recently
 * seen one it has not been told to leave alone:
 *
 * - **Replace it** merges the candidate into this machine (its runner rows,
 *   pins and pods move, with the collision rule of §13), then has the daemon
 *   re-create the local blocks of the moved engine runners — an engine runner
 *   needs nothing local but its folder, which the daemon allocates (§10).
 *   Own-agent runners cannot be rebuilt (their agent and folder were local);
 *   they stay offline, for "Set up again on this computer" or removal.
 * - **Keep it** leaves the old machine offline for the user to remove (PO-9)
 *   and is remembered, so the app does not ask about it again.
 *
 * Legacy desktop machines (`provisioning = desktop_bundled`) are not offered:
 * the migration (`desktop-migration`) merges those.
 */

import {
  compatibleHello,
  currentDiscovery,
  invoke,
  sessionGet,
  sessionPost,
  signInWorkspace,
  type Discovery,
  type LocalRunner,
} from "@/services/desktop-daemon";
import { mergeMachine, type MergeResult } from "@/services/desktop-migration";
import { dismissNotices, loadNotices, replaceKey, type ServerRunner } from "@/services/desktop-notices";

export interface ReplaceCandidate {
  id: string;
  label: string;
  host_label: string;
  display_name?: string;
  last_seen_at: string | null;
  created_at?: string | null;
  provisioning?: "manual" | "desktop_bundled";
  live_runner_count?: number;
}

export const replaceCandidatesPath = (machineId: string) =>
  `/api/v1/runner/dev-machines/${machineId}/replace-candidates/`;
export const MY_WORKSPACES_PATH = "/api/users/me/workspaces/";
export const RUNNERS_PATH = "/api/runners/";

/** The seams the flow touches, injectable for tests. */
export interface ReplaceDeps {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  get: <T>(path: string, params?: Record<string, string>) => Promise<T>;
  post: <T>(path: string, body: unknown) => Promise<T>;
  signInWorkspace: (workspaceSlug: string) => Promise<void>;
  discovery: () => Discovery | null;
}

const defaultDeps: ReplaceDeps = {
  invoke,
  get: sessionGet,
  post: sessionPost,
  signInWorkspace,
  discovery: currentDiscovery,
};

export interface ReplacePrompt {
  /** This computer's machine, which the candidate would be merged into. */
  machineId: string;
  candidate: ReplaceCandidate;
}

/**
 * The machine to ask about, or `null`: none, the daemon is not the app's, or
 * every candidate was already declined. Never throws.
 */
export async function findReplacePrompt(
  account: string,
  deps: ReplaceDeps = defaultDeps
): Promise<ReplacePrompt | null> {
  const machineId = compatibleHello(deps.discovery())?.machine_id;
  if (!machineId) return null;
  try {
    const { candidates } = await deps.get<{ candidates: ReplaceCandidate[] }>(replaceCandidatesPath(machineId));
    const { dismissed } = await loadNotices(account, deps.invoke);
    // The server orders them most recently seen first.
    const candidate = (candidates ?? []).find(
      (c) => c.id !== machineId && c.provisioning !== "desktop_bundled" && !dismissed.includes(replaceKey(c.id))
    );
    return candidate ? { machineId, candidate } : null;
  } catch {
    return null;
  }
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

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  const data = (error as { error?: string; detail?: string } | null) ?? null;
  return data?.error ?? data?.detail ?? String(error);
}

interface MovedEngine {
  workspace: string;
  project: string;
}

/** Which moved runners are engines, and in which workspace and project. */
async function movedEngines(moved: string[], deps: ReplaceDeps): Promise<MovedEngine[]> {
  if (!moved.length) return [];
  const wanted = new Set(moved);
  const workspaces = await deps.get<{ id: string; slug: string }[]>(MY_WORKSPACES_PATH);
  const engines: MovedEngine[] = [];
  for (const workspace of workspaces ?? []) {
    if (!wanted.size) break;
    // eslint-disable-next-line no-await-in-loop -- stops as soon as every moved runner is found
    const runners = await deps.get<ServerRunner[]>(RUNNERS_PATH, { workspace: workspace.id });
    for (const runner of runners ?? []) {
      if (!wanted.delete(runner.id)) continue;
      const project = runner.pod_detail?.project_identifier;
      if (runner.is_engine && project) engines.push({ workspace: workspace.slug, project });
    }
  }
  return engines;
}

/**
 * "Replace it": merge `prompt.candidate` into this machine, then re-create the
 * moved engine runners' local blocks on the daemon. The merge is idempotent
 * for the same target, so a retry after a partial failure is safe.
 */
export async function replaceMachine(prompt: ReplacePrompt, deps: ReplaceDeps = defaultDeps): Promise<ReplaceOutcome> {
  const hello = compatibleHello(deps.discovery());
  if (hello?.machine_id !== prompt.machineId) {
    throw new Error("Pi Dash on this computer changed meanwhile. Try again.");
  }
  const merge = await mergeMachine(deps.post, prompt.candidate.id, prompt.machineId);
  const moved = merge.moved_runner_ids ?? [];
  const engines = await movedEngines(moved, deps);
  const restored: string[] = [];
  const failed: { project: string; error: string }[] = [];
  let signedIn = new Set(hello.workspaces.filter((w) => w.signed_in).map((w) => w.slug));
  const local = await deps.invoke<LocalRunner[]>("daemon_runners");
  for (const { workspace, project } of engines) {
    const name = `${workspace}/${project}`;
    try {
      if (!signedIn.has(workspace)) {
        // eslint-disable-next-line no-await-in-loop -- the daemon's config lock serialises these anyway
        await deps.signInWorkspace(workspace);
        const now = compatibleHello(deps.discovery());
        signedIn = new Set([...signedIn, ...(now?.workspaces.filter((w) => w.signed_in).map((w) => w.slug) ?? [])]);
      }
      const served = local.some(
        (r) => r.agent === "engine" && r.workspace_slug === workspace && r.project_slug === project
      );
      if (!served) {
        // No working dir: the daemon allocates the folder (§10); the server's
        // reuse lookup returns the moved row, so the runner keeps its id.
        // eslint-disable-next-line no-await-in-loop -- one runner at a time
        await deps.invoke("daemon_engine_runner_add", { workspace, project });
      }
      restored.push(name);
    } catch (error) {
      failed.push({ project: name, error: errorText(error) });
    }
  }
  return { merge, restored, failed, ownAgentRunners: Math.max(0, moved.length - engines.length) };
}

/** "Keep it": leave the old machine, and never ask about it again. */
export async function declineReplace(
  account: string,
  prompt: ReplacePrompt,
  deps: Pick<ReplaceDeps, "invoke"> = defaultDeps
): Promise<void> {
  await dismissNotices(account, [replaceKey(prompt.candidate.id)], deps.invoke);
}

// --- The prompt's mount point -----------------------------------------------

type PromptListener = (prompt: { account: string; prompt: ReplacePrompt } | null) => void;
let promptListener: PromptListener | null = null;

/** The dialog registers here; `null` unregisters. */
export function registerReplacePrompt(listener: PromptListener | null) {
  promptListener = listener;
}

/** Find a candidate and, when there is one, show the prompt. Resolves whether it did. */
export async function offerReplace(account: string, deps: ReplaceDeps = defaultDeps): Promise<boolean> {
  const prompt = await findReplacePrompt(account, deps);
  if (!prompt || !promptListener) return false;
  promptListener({ account, prompt });
  return true;
}
