/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Moving this computer from older builds' private daemons to the shared one
 * (`.ai_design/built_engine_as_runner/design.md` §13, desktop steps 1–5).
 *
 * Older builds ran a daemon of their own per workspace, under the app's data,
 * each registered as a *legacy desktop machine*. For each such tree still on
 * disk (`legacy_scan`):
 *
 * 2. make sure the daemon holds a credential for the tree's workspace, then
 *    merge the legacy machine into the daemon's machine on the server — its
 *    runner rows, pins and pods move, ids kept;
 * 3. `EngineRunnerAdd` each legacy engine block with its project and working
 *    dir; the server's reuse lookup returns the merged row, so the runner
 *    keeps its id and its folder;
 * 4–5. `legacy_finish`: move the account's engine home into the daemon's and
 *    delete the tree's config, credentials and model token. Task folders stay.
 *
 * Before step 5 it records the tree's one-time notices (`desktop-notices`):
 * each legacy folder left unused because the daemon already ran the engine for
 * that project (the merge's collision rule), and each project whose re-added
 * engine runner the server made a pod member on upgrade (PO-11).
 *
 * Step 1 is the caller's: nothing runs unless the daemon is the app's
 * (`compatible`). Blocked — no daemon, another account, the service declined —
 * the trees stay and every later launch tries again. A finished tree is gone
 * from the scan, so the migration never repeats; a half-finished one resumes
 * where it stopped (the merge is idempotent for the same target, and a project
 * the daemon already runs the engine for is not added twice).
 */

import {
  compatibleHello,
  currentDiscovery,
  daemonAccountKey,
  invoke,
  sessionGet,
  sessionPost,
  signInWorkspace,
  type Discovery,
  type EngineRunnerInfo,
  type HelloInfo,
  type LocalRunner,
} from "@/services/desktop-daemon";
import { emptyNotices, hasNotices, recordNotices, runnerPath, type ServerRunner } from "@/services/desktop-notices";

export interface LegacyEngineRunner {
  name: string;
  runner_id: string | null;
  project: string;
  working_dir: string;
}

export interface LegacyTree {
  workspace: string;
  dir: string;
  cloud_url: string | null;
  dev_machine_id: string | null;
  engine_runners: LegacyEngineRunner[];
}

export const mergePath = (source: string) => `/api/v1/runner/dev-machines/${source}/merge-into/`;

/** `POST merge-into/`'s answer (`machine_identity.merge_dev_machine`). */
export interface MergeResult {
  source_dev_machine_id: string;
  target_dev_machine_id: string;
  merged_at?: string;
  moved_runner_ids: string[];
  collisions: { revoked_runner_id: string; survivor_runner_id: string }[];
}

/** Merge machine `source` into `target` (design §13; also the §4.10 replace). */
export function mergeMachine(
  post: <T>(path: string, body: unknown) => Promise<T>,
  source: string,
  target: string
): Promise<MergeResult> {
  return post<MergeResult>(mergePath(source), { target });
}

export type MigrationOutcome =
  | { status: "nothing_to_do" }
  | { status: "blocked"; reason: Discovery["kind"] | "unknown" }
  | { status: "done"; migrated: string[]; failed: { workspace: string; error: string }[] };

/** The seams the migration touches, injectable for tests. */
export interface MigrationDeps {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  post: <T>(path: string, body: unknown) => Promise<T>;
  get: <T>(path: string) => Promise<T>;
  signInWorkspace: (workspaceSlug: string) => Promise<void>;
  discovery: () => Discovery | null;
}

const defaultDeps: MigrationDeps = {
  invoke,
  post: sessionPost,
  get: (path) => sessionGet(path),
  signInWorkspace,
  discovery: currentDiscovery,
};

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  const data = (error as { error?: string; detail?: string } | null) ?? null;
  return data?.error ?? data?.detail ?? String(error);
}

async function migrateTree(tree: LegacyTree, account: string, hello: HelloInfo, deps: MigrationDeps): Promise<void> {
  // Step 2a: the daemon must hold this workspace's credential.
  let current = hello;
  if (!current.workspaces.some((w) => w.slug === tree.workspace && w.signed_in)) {
    await deps.signInWorkspace(tree.workspace);
    current = compatibleHello(deps.discovery()) ?? current;
  }
  // Step 2b: merge the legacy machine into the daemon's.
  const target = current.machine_id;
  if (tree.dev_machine_id && target && tree.dev_machine_id !== target) {
    await mergeMachine(deps.post, tree.dev_machine_id, target);
  }
  // Step 3: the engine runners, unless the daemon already runs the engine for
  // that project (a resumed migration, or a CLI that turned it on first).
  const runners = await deps.invoke<LocalRunner[]>("daemon_runners");
  // Case-insensitive, as the daemon refuses a second engine for a project
  // (`runner/src/engine/provision.rs`): `web` and `WEB` are one project.
  const servedBy = (project: string) =>
    runners.find(
      (r) =>
        r.agent === "engine" &&
        r.workspace_slug === tree.workspace &&
        r.project_slug?.toLowerCase() === project.toLowerCase()
    );
  const notices = emptyNotices();
  // The tree's runners now on the daemon: re-added here, or by an earlier
  // run of a migration that stopped before step 5 (same folder).
  const added: { runnerId: string; project: string }[] = [];
  for (const block of tree.engine_runners) {
    const served = servedBy(block.project);
    if (served) {
      if (served.working_dir === block.working_dir) {
        added.push({ runnerId: served.runner_id, project: block.project });
      } else if (block.runner_id && served.runner_id === block.runner_id) {
        // The same runner, re-added by the daemon's reconcile with a fresh
        // folder after an interrupted migration: it keeps its own folder
        // (§13 step 3), so point it back there.
        // eslint-disable-next-line no-await-in-loop -- one runner at a time
        await deps.invoke("legacy_restore_folder", { runner: served.name, workingDir: block.working_dir });
        added.push({ runnerId: served.runner_id, project: block.project });
      } else {
        // The daemon's own runner survived the merge; the legacy folder is unused.
        notices.legacy_folders.push({ workspace: tree.workspace, project: block.project, path: block.working_dir });
      }
      continue;
    }
    // eslint-disable-next-line no-await-in-loop -- one runner at a time, in config order
    const info = await deps.invoke<EngineRunnerInfo | undefined>("daemon_engine_runner_add", {
      workspace: tree.workspace,
      project: block.project,
      workingDir: block.working_dir,
    });
    const runnerId = info?.runner_id ?? block.runner_id;
    if (runnerId) added.push({ runnerId, project: block.project });
  }
  // PO-11: engines that became pod members on upgrade. Engines start
  // `pinned_only`, so a re-added one that takes pod work was switched by the
  // server's migration. A lookup that fails only loses its notice.
  for (const { runnerId, project } of added) {
    // eslint-disable-next-line no-await-in-loop -- a handful of runners per tree
    const row = await deps.get<ServerRunner>(runnerPath(runnerId)).catch(() => null);
    if (row?.pod_dispatch === "pod_member") {
      notices.pod_member_projects.push({ runner_id: runnerId, workspace: tree.workspace, project });
    }
  }
  if (hasNotices(notices)) await recordNotices(account, notices, deps.invoke);
  // Steps 4 and 5.
  await deps.invoke("legacy_finish", { dir: tree.dir, account, daemonAccount: daemonAccountKey(current) ?? null });
}

/**
 * Run the desktop migration for the signed-in `account`. Never throws: each
 * tree that fails is reported and left for the next launch.
 */
export function migrateLegacyTrees(account: string, deps: MigrationDeps = defaultDeps): Promise<MigrationOutcome> {
  // One run at a time: a window focus or the minute timer while a run is in
  // flight joins it instead of starting a second (duplicate device grants,
  // a spurious "already on").
  if (inFlight) return inFlight;
  const run = runMigration(account, deps).finally(() => {
    if (inFlight === run) inFlight = null;
  });
  inFlight = run;
  return run;
}

let inFlight: Promise<MigrationOutcome> | null = null;

async function runMigration(account: string, deps: MigrationDeps): Promise<MigrationOutcome> {
  let trees: LegacyTree[];
  try {
    trees = await deps.invoke<LegacyTree[]>("legacy_scan");
  } catch {
    return { status: "nothing_to_do" };
  }
  if (!trees.length) return { status: "nothing_to_do" };
  const discovery = deps.discovery();
  const hello = compatibleHello(discovery);
  if (!hello) return { status: "blocked", reason: discovery?.kind ?? "unknown" };
  const migrated: string[] = [];
  const failed: { workspace: string; error: string }[] = [];
  for (const tree of trees) {
    try {
      // eslint-disable-next-line no-await-in-loop -- trees share the daemon's config lock
      await migrateTree(tree, account, hello, deps);
      migrated.push(tree.workspace);
    } catch (error) {
      failed.push({ workspace: tree.workspace, error: errorText(error) });
    }
  }
  return { status: "done", migrated, failed };
}
