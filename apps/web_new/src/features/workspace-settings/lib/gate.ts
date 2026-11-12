// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings gate decisions (parity/specs/workspace-settings.md §1, §2,
// §36; WSET-002/036). Pure so the route and its tests share one answer:
// the shell renders a denial view on the same address and never
// redirects. Only addresses inside the settings subtree are gated.

import type { Permissions } from "../../../core/session/index.js";
import {
  canOpenWorkspaceSettingsSection,
  resolveWorkspaceSettingsSection,
  type WorkspaceSettingsSectionKey,
} from "./sections.js";

export type WorkspaceSettingsGate =
  | { status: "denied"; section: WorkspaceSettingsSectionKey | null }
  | { status: "allowed"; section: WorkspaceSettingsSectionKey };

/**
 * Decide the gate for a settings address. A settings address whose
 * section name is unknown denies with no section (no title). Addresses
 * outside `/{workspace}/settings` are not this gate's business.
 */
export function decideWorkspaceSettingsGate(
  pathname: string,
  permissions: Pick<Permissions, "canEdit" | "isAdmin">
): WorkspaceSettingsGate | null {
  const [head, settings] = pathname.split("/").filter(Boolean);
  if (head === undefined || settings !== "settings") return null;
  const section = resolveWorkspaceSettingsSection(pathname);
  if (section === null) return { status: "denied", section: null };
  return canOpenWorkspaceSettingsSection(section, permissions)
    ? { status: "allowed", section }
    : { status: "denied", section };
}
