// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports-guide seam (parity/specs/workspace-settings.md §33/§41,
// WSET-033). The workspace-settings exports entry owns the frame,
// explanation and gate; the exports area owns the form, history,
// downloads and polling. It registers one guide component here, and this
// module only holds that registration — no export behavior, request or
// copy lives in this slice.

import * as React from "react";

export interface WorkspaceExportsGuideProps {
  workspaceSlug: string;
}

export type WorkspaceExportsGuideComponent = (props: WorkspaceExportsGuideProps) => React.ReactNode;

let guide: WorkspaceExportsGuideComponent | null = null;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

/**
 * Register the exports area's guide. Returns an unmount cleanup that
 * removes only this registration, so a stale owner cannot clear a newer
 * guide.
 */
export function registerWorkspaceExportsGuide(component: WorkspaceExportsGuideComponent): () => void {
  guide = component;
  emit();
  return () => {
    if (guide !== component) return;
    guide = null;
    emit();
  };
}

export function getWorkspaceExportsGuide(): WorkspaceExportsGuideComponent | null {
  return guide;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The registered guide, re-rendering when the exports area mounts it. */
export function useWorkspaceExportsGuide(): WorkspaceExportsGuideComponent | null {
  return React.useSyncExternalStore(subscribe, getWorkspaceExportsGuide, getWorkspaceExportsGuide);
}

/** Clears the registration. Tests only; production unregisters on unmount. */
export function resetWorkspaceExportsGuide(): void {
  guide = null;
  emit();
}
