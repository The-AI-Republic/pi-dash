/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The app's one-time notices (`.ai_design/built_engine_as_runner/design.md`
 * §13), kept in its own per-account settings (`account_settings.rs`) so they
 * survive sign-out and restarts and are never shown twice:
 *
 * - **legacy folders** — task folders of older builds the migration left on
 *   disk because the daemon already ran the engine for that project (the
 *   merge's collision rule keeps the daemon's runner and its folder);
 * - **pod members** — projects whose engine on this computer became a pod
 *   member on upgrade (PO-11), so the owner's "Runner" issues there can go to
 *   the engine and spend the wallet, also while the app is closed. Each comes
 *   with a switch to turn pod work off (`pod_dispatch = pinned_only`).
 *
 * A notice stays until dismissed; a dismissed one is remembered by its key,
 * so a retried migration that records it again shows nothing. The same store
 * remembers each declined "Is this <label>? Replace it" prompt (§4.10).
 */

import { invoke as hostInvoke, sessionGet, sessionPatch } from "@/services/desktop-daemon";

export interface LegacyFolderNotice {
  workspace: string;
  project: string;
  path: string;
}

export interface PodMemberNotice {
  runner_id: string;
  workspace: string;
  project: string;
}

export interface Notices {
  legacy_folders: LegacyFolderNotice[];
  pod_member_projects: PodMemberNotice[];
}

export interface NoticesView {
  notices: Notices;
  dismissed: string[];
}

export const folderKey = (n: LegacyFolderNotice) => `folder:${n.path}`;
export const podKey = (n: PodMemberNotice) => `pod:${n.runner_id}`;
export const replaceKey = (machineId: string) => `replace:${machineId}`;

export const emptyNotices = (): Notices => ({ legacy_folders: [], pod_member_projects: [] });

export function hasNotices(n: Notices): boolean {
  return n.legacy_folders.length > 0 || n.pod_member_projects.length > 0;
}

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

const listeners = new Set<() => void>();

/** Subscribe to "the notices changed" (recorded or dismissed); returns the unsubscribe. */
export function subscribeNotices(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function emit() {
  for (const listener of listeners) listener();
}

export function loadNotices(account: string, invoke: Invoke = hostInvoke): Promise<NoticesView> {
  return invoke<NoticesView>("desktop_notices_load", { account });
}

/** Record new notices; ones already pending or dismissed are dropped by the host. */
export async function recordNotices(account: string, notices: Notices, invoke: Invoke = hostInvoke) {
  const view = await invoke<NoticesView>("desktop_notices_record", { account, notices });
  emit();
  return view;
}

/** Dismiss for good (a notice) or decline (a replace prompt). */
export async function dismissNotices(account: string, keys: string[], invoke: Invoke = hostInvoke) {
  const view = await invoke<NoticesView>("desktop_notices_dismiss", { account, keys });
  emit();
  return view;
}

// --- Pod work (PO-11) --------------------------------------------------------

export type PodDispatch = "pinned_only" | "pod_member";

export const runnerPath = (runnerId: string) => `/api/runners/${runnerId}/`;

/** The server's view of one runner, as much of it as the notices read. */
export interface ServerRunner {
  id: string;
  is_engine?: boolean;
  pod_dispatch?: PodDispatch;
  pod_detail?: { project_identifier?: string } | null;
}

export function getRunner(runnerId: string): Promise<ServerRunner> {
  return sessionGet<ServerRunner>(runnerPath(runnerId));
}

/** The notice's switch: pod work on (`pod_member`) or off (`pinned_only`). */
export function setPodWork(runnerId: string, on: boolean): Promise<ServerRunner> {
  return sessionPatch<ServerRunner>(runnerPath(runnerId), { pod_dispatch: on ? "pod_member" : "pinned_only" });
}
