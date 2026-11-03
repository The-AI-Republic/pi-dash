// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar identity resolution (parity/specs/workspace-settings.md §1,
// WSET-001). The details read is authoritative while it answers; a
// failed read leaves the listed workspace row in charge, and the slug
// is the last resort so the frame never renders nameless.

export interface WorkspaceSettingsIdentity {
  workspaceName: string;
  logoLeg: string | null;
}

export function resolveWorkspaceSettingsIdentity(input: {
  workspaceSlug: string;
  details: { name: string; logo_url: string | null } | undefined;
  listed: { name: string; logo_url: string | null } | undefined;
}): WorkspaceSettingsIdentity {
  const { workspaceSlug, details, listed } = input;
  return {
    workspaceName: details?.name ?? listed?.name ?? workspaceSlug,
    logoLeg: details?.logo_url ?? listed?.logo_url ?? null,
  };
}
