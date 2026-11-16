// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings tab titles (parity/specs/workspace-settings.md §2, WSET-007).
// Allowed sections carry the workspace name plus the section name; the
// webhook detail address names "Webhook". Denied sections leave the tab
// title empty. Pure: the shell owns the document head.

import { findWorkspaceSettingsSection, type WorkspaceSettingsSectionKey } from "./sections.js";

export interface WorkspaceSettingsTitleInput {
  /** Current workspace display name. */
  workspaceName: string;
  section: WorkspaceSettingsSectionKey;
  /** Whether the address is a webhook detail (not the list). */
  webhookDetail: boolean;
}

export function workspaceSettingsTitle({ workspaceName, section, webhookDetail }: WorkspaceSettingsTitleInput): string {
  const name = webhookDetail ? "Webhook" : findWorkspaceSettingsSection(section).title;
  return `${workspaceName} - ${name}`;
}

/** True when a webhooks address has an id segment after the list. */
export function isWorkspaceWebhookDetailPath(pathname: string): boolean {
  const [head, settings, section, webhookId] = pathname.split("/").filter(Boolean);
  return head !== undefined && settings === "settings" && section === "webhooks" && webhookId !== undefined;
}
