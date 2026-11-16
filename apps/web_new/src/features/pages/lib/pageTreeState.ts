// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// In-session tree expansion (parity/specs/pages.md §12.2, PAGE-085).
// Expanding a parent is a view preference, not server state: it survives a
// route change within the session and is deliberately forgotten on a full
// reload. One module-level store per workspace/project keeps the list and
// anything that re-mounts it in step, without threading state through the
// router.

import * as React from "react";

const expanded = new Map<string, Set<string>>();
const listeners = new Set<() => void>();

function scopeKey(workspaceSlug: string, projectId: string): string {
  return `${workspaceSlug}/${projectId}`;
}

function emit(): void {
  for (const listener of listeners) listener();
}

/** Current expanded parent ids for one project (a stable set per scope). */
export function expandedPageIds(workspaceSlug: string, projectId: string): ReadonlySet<string> {
  let set = expanded.get(scopeKey(workspaceSlug, projectId));
  if (set === undefined) {
    set = new Set();
    expanded.set(scopeKey(workspaceSlug, projectId), set);
  }
  return set;
}

/**
 * Expand or collapse one parent; the change is broadcast to every reader.
 * Each change replaces the scope's set, because React's external-store
 * snapshot must be a new reference for a re-render to happen.
 */
export function setPageExpanded(workspaceSlug: string, projectId: string, pageId: string, open: boolean): void {
  const current = expandedPageIds(workspaceSlug, projectId);
  if (open === current.has(pageId)) return;
  const next = new Set(current);
  if (open) next.add(pageId);
  else next.delete(pageId);
  expanded.set(scopeKey(workspaceSlug, projectId), next);
  emit();
}

/** Toggle one parent and report the new state. */
export function togglePageExpanded(workspaceSlug: string, projectId: string, pageId: string): boolean {
  const open = !expandedPageIds(workspaceSlug, projectId).has(pageId);
  setPageExpanded(workspaceSlug, projectId, pageId, open);
  return open;
}

/** Collapse every parent in one project. Tests and project switches only. */
export function resetPageExpansion(workspaceSlug?: string, projectId?: string): void {
  if (workspaceSlug === undefined || projectId === undefined) expanded.clear();
  else expanded.delete(scopeKey(workspaceSlug, projectId));
  emit();
}

/** Subscribe a component to expansion changes. */
export function subscribePageExpansion(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Read the current expansion inside React; re-renders on every change. */
export function usePageExpansion(workspaceSlug: string, projectId: string): ReadonlySet<string> {
  return React.useSyncExternalStore(
    subscribePageExpansion,
    () => expandedPageIds(workspaceSlug, projectId),
    () => expandedPageIds(workspaceSlug, projectId)
  );
}
