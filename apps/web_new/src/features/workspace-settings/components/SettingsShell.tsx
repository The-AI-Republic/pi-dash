// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-settings shell (parity/specs/workspace-settings.md §1/§2,
// WSET-001/002). Composes the desktop sidebar, the narrow bar/drawer,
// the identity block and the registered page body inside one gate. The
// shell owns no page content: slices register theirs through
// lib/registry.ts, so the gate, addresses and navigation stay singular.
// Cross-cutting contracts live in this frame too: no live re-reads
// (§38), no drag/import/file affordances (§39), no desktop hooks (§40).

import * as React from "react";

import type { Permissions } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import {
  findWorkspaceSettingsSection,
  visibleWorkspaceSettingsGroups,
  type WorkspaceSettingsSectionKey,
} from "../lib/sections.js";
import { workspaceSettingsTitle } from "../lib/title.js";
import { SettingsIdentity } from "./SettingsIdentity.js";
import { SettingsMobileNav } from "./SettingsMobileNav.js";
import { SettingsNavigation } from "./SettingsNavigation.js";
import { SettingsPageFrame } from "./SettingsPageFrame.js";

export interface SettingsShellProps {
  workspaceSlug: string;
  workspaceName: string;
  logoUrl: string | null;
  role: number | null;
  permissions: Permissions;
  section: WorkspaceSettingsSectionKey;
  webhookDetail: boolean;
  onNavigate: (href: string) => void;
  /** The section outlet; it renders the placeholder until a slice registers. */
  body: React.ReactNode;
}

export function SettingsShell({
  workspaceSlug,
  workspaceName,
  logoUrl,
  role,
  permissions,
  section,
  webhookDetail,
  onNavigate,
  body,
}: SettingsShellProps): React.ReactElement {
  const groups = visibleWorkspaceSettingsGroups(workspaceSlug, permissions);
  const current = findWorkspaceSettingsSection(section);
  // The sidebar names the integrations entry with its provider suffix
  // (§1); the frame and the tab title use the section name (§2).
  const frameTitle = section === "integrations" ? current.title : current.label;

  useDocumentHead({
    title: workspaceSettingsTitle({ workspaceName, section, webhookDetail }),
    robots: "noindex, nofollow",
  });

  return (
    <div data-testid="workspace-settings-shell" className="flex h-full min-h-0 bg-(--bg) text-(--text)">
      <aside
        aria-label="Settings sidebar"
        data-testid="workspace-settings-sidebar"
        className="hidden h-full w-64 shrink-0 flex-col border-r border-(--border) bg-(--bg) min-[48rem]:flex"
      >
        <SettingsIdentity
          workspaceSlug={workspaceSlug}
          workspaceName={workspaceName}
          logoUrl={logoUrl}
          role={role}
          onBack={() => {
            onNavigate(`/${workspaceSlug}`);
          }}
        />
        <div className="min-h-0 flex-1 overflow-y-auto p-(--space-2)">
          <SettingsNavigation
            idPrefix="workspace-settings-group"
            groups={groups}
            activeSection={section}
            onNavigate={onNavigate}
          />
        </div>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <SettingsMobileNav
          groups={groups}
          activeSection={section}
          activeLabel={webhookDetail ? "Webhook" : current.label}
          onNavigate={onNavigate}
        />
        <SettingsPageFrame title={webhookDetail ? "Webhook" : frameTitle}>{body}</SettingsPageFrame>
      </div>
    </div>
  );
}
