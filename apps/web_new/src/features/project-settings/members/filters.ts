// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-project member filter memory (parity/specs/project-settings-members.md
// §"Search, filter and sort"; PSET-020/021, pin NEWFRONT-409). The role
// filter and column sort survive moving between settings tabs inside one
// session and reset on a full reload, so they live in a module-level map
// keyed by project rather than in route state or storage. The search box
// is deliberately not persisted: the spec only carries filter and sort.

import * as React from "react";

import { EMPTY_MEMBER_ROSTER_FILTERS, type MemberRosterFilters } from "./roster.js";

type Listener = () => void;

const filtersByProject = new Map<string, MemberRosterFilters>();
const listenersByProject = new Map<string, Set<Listener>>();

/** Current persisted filters for a project; the shared empty value by default. */
export function readMemberFilters(projectId: string): MemberRosterFilters {
  return filtersByProject.get(projectId) ?? EMPTY_MEMBER_ROSTER_FILTERS;
}

/** Replace the persisted filters for a project and notify its readers. */
export function writeMemberFilters(projectId: string, filters: MemberRosterFilters): void {
  filtersByProject.set(projectId, filters);
  const listeners = listenersByProject.get(projectId);
  if (listeners === undefined) return;
  for (const listener of listeners) listener();
}

/** Drop every remembered choice; used by tests and on workspace sign-out. */
export function clearMemberFilters(): void {
  filtersByProject.clear();
}

function subscribe(projectId: string, listener: Listener): () => void {
  let listeners = listenersByProject.get(projectId);
  if (listeners === undefined) {
    listeners = new Set();
    listenersByProject.set(projectId, listeners);
  }
  listeners.add(listener);
  // The empty set stays behind: one entry per project touched this session
  // is cheaper than tracking whether a later reader may arrive.
  return () => {
    listeners.delete(listener);
  };
}

/**
 * The persisted role filter and sort for one project, with a setter that
 * keeps the current search box untouched. A remount inside the same
 * session reads the previous choice back.
 */
export function useMemberFilters(projectId: string): [MemberRosterFilters, (filters: MemberRosterFilters) => void] {
  const filters = React.useSyncExternalStore(
    React.useCallback((listener) => subscribe(projectId, listener), [projectId]),
    React.useCallback(() => readMemberFilters(projectId), [projectId]),
    React.useCallback(() => readMemberFilters(projectId), [projectId])
  );
  const write = React.useCallback(
    (next: MemberRosterFilters) => {
      writeMemberFilters(projectId, next);
    },
    [projectId]
  );
  return [filters, write];
}
