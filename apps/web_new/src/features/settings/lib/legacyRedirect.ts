// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Router glue for the legacy forwards (PROF-026; main#670): the final
// address for a legacy pathname, with the query string and the fragment
// kept, or null when the address belongs to no legacy family. The old
// app carried `location.hash` too, so a bookmarked assistant link such as
// `/settings/profile/ai-assistant#voice-dictation` still opens the
// dictation block. The current workspace is the
// user's last one when they still belong to it, else the remembered
// fallback, matching where the app lands after sign-in. A user who belongs
// to no workspace has nowhere to host account settings and continues to
// workspace creation instead.

import type { WorkspaceList } from "@pidash/api-client";
import { CREATE_WORKSPACE_PATH, selectWorkspace } from "../../../core/session/index.js";
import { accountSettingsHref } from "./accountHref.js";
import { legacySettingsTarget } from "./legacyForwards.js";

/** Inputs the current-workspace resolution needs. */
export interface LegacyWorkspaceInputs {
  lastSlug: string | null;
  fallbackSlug: string | null;
  workspaces: WorkspaceList | undefined;
}

/** The workspace a workspace-less legacy address lands in, or null. */
export function legacyCurrentWorkspaceSlug(inputs: LegacyWorkspaceInputs): string | null {
  for (const candidate of [inputs.lastSlug, inputs.fallbackSlug]) {
    if (candidate === null) continue;
    const found = selectWorkspace(inputs.workspaces, candidate);
    if (found !== undefined) return found.slug;
  }
  return null;
}

/**
 * The address a legacy pathname forwards to, or null outside the
 * families. The tab lands in the reader's current workspace, which the
 * inputs resolve.
 */
export function legacySettingsRedirect(
  pathname: string,
  search: string,
  current: LegacyWorkspaceInputs,
  hash = ""
): string | null {
  const tab = legacySettingsTarget(pathname);
  if (tab === null) return null;
  const workspaceSlug = legacyCurrentWorkspaceSlug(current);
  if (workspaceSlug === null) return CREATE_WORKSPACE_PATH;
  return accountSettingsHref(workspaceSlug, tab, search, hash);
}
