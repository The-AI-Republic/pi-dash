// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shell entity-peek request seam (parity/specs/pages.md §13.2, PAGE-043).
// A page body emits one normalised reference; the shell owns the panel and
// decides which entity kinds it can open. Pages never imports an area's
// detail surface, and the host never depends on page internals.

/** One stored entity reference the body wants to preview. */
export interface EntityPeekRequest {
  entityName: string;
  entityIdentifier: string;
  projectIdentifier: string;
  workspaceIdentifier: string;
  projectId: string | null;
  label: string;
}

const listeners = new Set<(request: EntityPeekRequest) => void>();

/** Register one shell host; returns its disposer. */
export function subscribeEntityPeek(listener: (request: EntityPeekRequest) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Ask the mounted shell to open the matching peek, if it knows the kind. */
export function requestEntityPeek(request: EntityPeekRequest): void {
  for (const listener of listeners) listener(request);
}

/** Test-only reset; production hosts dispose through their subscription. */
export function resetEntityPeek(): void {
  listeners.clear();
}
