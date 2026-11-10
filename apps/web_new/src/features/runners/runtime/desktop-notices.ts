// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The app's one-time notices (design §13), kept in the host's per-account
// settings so they survive sign-out and restarts and are never shown twice:
//
// - legacy folders — task folders of older builds the migration left on
//   disk because the daemon already ran the engine for that project (the
//   merge's collision rule keeps the daemon's runner and its folder);
// - pod members — projects whose engine on this computer became a pod
//   member on upgrade (PO-11), so the owner's "Runner" issues there can go
//   to the engine and spend the wallet, also while the app is closed. Each
//   comes with a switch to turn pod work off (`pod_dispatch = pinned_only`).
//
// A notice stays until dismissed; a dismissed one is remembered by its key,
// so a retried migration that records it again shows nothing. The same store
// remembers each declined "Is this <label>? Replace it" prompt (§4.10).

import { getRunner, updateRunner, type ApiClient, type Runner } from "@pidash/api-client";

import type {
  AgentRuntimeApi,
  DesktopNotices,
  LegacyFolderNotice,
  NoticesView,
  PodMemberNotice,
} from "../../../core/platform/index.js";

export type { DesktopNotices, LegacyFolderNotice, NoticesView, PodMemberNotice };

export const folderKey = (notice: LegacyFolderNotice): string => `folder:${notice.path}`;
export const podKey = (notice: PodMemberNotice): string => `pod:${notice.runner_id}`;
export const replaceKey = (machineId: string): string => `replace:${machineId}`;

export const emptyNotices = (): DesktopNotices => ({ legacy_folders: [], pod_member_projects: [] });

export function hasNotices(notices: DesktopNotices): boolean {
  return notices.legacy_folders.length > 0 || notices.pod_member_projects.length > 0;
}

const listeners = new Set<() => void>();

/** Subscribe to "the notices changed" (recorded or dismissed); returns the unsubscribe. */
export function subscribeNotices(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function emit(): void {
  for (const listener of listeners) listener();
}

/** Pending notices and dismissed keys for an account. */
export function loadNotices(account: string, runtime: AgentRuntimeApi): Promise<NoticesView> {
  return runtime.loadNotices(account);
}

/** Record new notices; ones already pending or dismissed are dropped by the host. */
export async function recordNotices(
  account: string,
  notices: DesktopNotices,
  runtime: AgentRuntimeApi
): Promise<NoticesView> {
  const view = await runtime.recordNotices(account, notices);
  emit();
  return view;
}

/** Dismiss for good (a notice) or decline (a replace prompt). */
export async function dismissNotices(account: string, keys: string[], runtime: AgentRuntimeApi): Promise<NoticesView> {
  const view = await runtime.dismissNotices(account, keys);
  emit();
  return view;
}

// --- Pod work (PO-11) -------------------------------------------------------

/** The server's view of one runner, as much of it as the notices read. */
export type ServerRunner = Runner;

export function getServerRunner(client: ApiClient, runnerId: string): Promise<Runner> {
  return getRunner(client, runnerId);
}

/** The notice's switch: pod work on (`pod_member`) or off (`pinned_only`). */
export function setPodWork(client: ApiClient, runnerId: string, on: boolean): Promise<Runner> {
  return updateRunner(client, runnerId, { pod_dispatch: on ? "pod_member" : "pinned_only" });
}
