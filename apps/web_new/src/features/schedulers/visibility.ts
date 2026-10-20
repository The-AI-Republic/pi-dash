// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-definition visibility for parity/specs/agents.md §5 (AGT-016). The
// rail's hidden set is a device-local preference scoped to one project:
// nothing reaches the API. It is read once per mount, so a second tab
// opens with the choice already applied while open tabs stay put.

import * as React from "react";

const STORAGE_PREFIX = "pidash:scheduler-calendar-hidden:";

export function visibilityStorageKey(workspaceSlug: string, projectId: string): string {
  return `${STORAGE_PREFIX}${workspaceSlug}:${projectId}`;
}

/** Read the hidden scheduler ids for a project; unknown shapes read empty. */
export function readHiddenSchedulers(
  storage: Pick<Storage, "getItem"> | null,
  workspaceSlug: string,
  projectId: string
): string[] {
  if (storage === null) return [];
  let raw: string | null;
  try {
    raw = storage.getItem(visibilityStorageKey(workspaceSlug, projectId));
  } catch {
    return [];
  }
  if (raw === null) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((entry): entry is string => typeof entry === "string");
  } catch {
    return [];
  }
}

export function writeHiddenSchedulers(
  storage: Pick<Storage, "setItem"> | null,
  workspaceSlug: string,
  projectId: string,
  hidden: readonly string[]
): void {
  if (storage === null) return;
  try {
    storage.setItem(visibilityStorageKey(workspaceSlug, projectId), JSON.stringify([...hidden]));
  } catch {
    // A full or blocked storage area leaves the choice in memory only.
  }
}

/**
 * The page's storage area, or null where there is none (server rendering,
 * or a browser that refuses the getter under its privacy settings).
 */
export function browserStorage(scope: { localStorage: Storage } | null = pageScope()): Storage | null {
  try {
    return scope === null ? null : scope.localStorage;
  } catch {
    return null;
  }
}

/** The page's global scope, or null outside a browser (server render, node tests). */
function pageScope(): { localStorage: Storage } | null {
  return globalThis.window ?? null;
}

export interface VisibilityState {
  hidden: ReadonlySet<string>;
  isVisible: (schedulerId: string) => boolean;
  toggle: (schedulerId: string) => void;
  showAll: () => void;
  hideAll: (schedulerIds: readonly string[]) => void;
}

/**
 * The rail's hidden set for one project. It starts from storage on mount
 * and writes every change back, so reloads and new tabs see it.
 */
export function useSchedulerVisibility(workspaceSlug: string, projectId: string): VisibilityState {
  const [hidden, setHidden] = React.useState<ReadonlySet<string>>(
    () => new Set(readHiddenSchedulers(browserStorage(), workspaceSlug, projectId))
  );

  const commit = React.useCallback(
    (next: ReadonlySet<string>) => {
      setHidden(next);
      writeHiddenSchedulers(browserStorage(), workspaceSlug, projectId, [...next]);
    },
    [workspaceSlug, projectId]
  );

  const toggle = React.useCallback(
    (schedulerId: string) => {
      const next = new Set(hidden);
      if (next.has(schedulerId)) next.delete(schedulerId);
      else next.add(schedulerId);
      commit(next);
    },
    [hidden, commit]
  );

  const showAll = React.useCallback(() => commit(new Set()), [commit]);
  const hideAll = React.useCallback((ids: readonly string[]) => commit(new Set(ids)), [commit]);

  return {
    hidden,
    isVisible: (schedulerId: string) => !hidden.has(schedulerId),
    toggle,
    showAll,
    hideAll,
  };
}
