// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace member directory behind rendered mentions (parity/specs/pages.md
// §6.4, PAGE-041). Stored mentions keep the name they were written with, but
// the live chip must show the person's current display name when the directory
// still knows them. The directory is a small external store so an editor node
// view and a read surface can both resolve names without either owning a query.

/** One member the directory can resolve a mention against. */
export interface MentionDirectoryEntry {
  id: string;
  name: string;
}

interface MentionDirectoryState {
  entries: ReadonlyMap<string, string>;
  currentUserId: string | null;
}

let state: MentionDirectoryState = { entries: new Map(), currentUserId: null };
let version = 0;
const listeners = new Set<() => void>();

/** Publish the workspace directory and signed-in member for mention chips. */
export function setMentionDirectory(entries: readonly MentionDirectoryEntry[], currentUserId: string | null): void {
  state = {
    entries: new Map(entries.map((entry) => [entry.id, entry.name])),
    currentUserId,
  };
  version += 1;
  for (const listener of listeners) listener();
}

/** Clear the directory when no page editor is mounted. */
export function resetMentionDirectory(): void {
  setMentionDirectory([], null);
}

/** A stable snapshot version for React's external-store subscription. */
export function mentionDirectoryVersion(): number {
  return version;
}

/** Subscribe to directory changes; node views repaint in place. */
export function subscribeMentionDirectory(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The current display name for a stored mention, falling back to its writer's name. */
export function mentionNameFor(id: string | null | undefined, storedName: string | null | undefined): string {
  const resolved = id === null || id === undefined ? undefined : state.entries.get(id);
  if (resolved !== undefined && resolved.trim() !== "") return resolved;
  return storedName?.trim() ?? "";
}

/** Whether a mention names the signed-in member. */
export function isCurrentMentionUser(id: string | null | undefined): boolean {
  return id !== null && id !== undefined && state.currentUserId !== null && id === state.currentUserId;
}
