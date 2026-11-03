// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings sidebar identity (parity/specs/workspace-settings.md §1,
// WSET-001): a back control to the workspace home, the logo or initial
// fallback, the workspace name and the viewer's role. No plan pill
// renders in the open-source checkout.

import { ArrowLeft } from "lucide-react";
import * as React from "react";

import { WorkspaceLogo } from "../../../shared/shell/index.js";
import { workspaceSettingsRoleLabel } from "../lib/sections.js";

export interface SettingsIdentityProps {
  workspaceSlug: string;
  workspaceName: string;
  logoUrl: string | null;
  role: number | null;
  onBack: () => void;
}

export function SettingsIdentity({
  workspaceSlug,
  workspaceName,
  logoUrl,
  role,
  onBack,
}: SettingsIdentityProps): React.ReactElement {
  const roleLabel = workspaceSettingsRoleLabel(role);
  return (
    <div className="flex flex-col gap-(--space-3) border-b border-(--border) px-(--space-3) py-(--space-3)">
      <a
        href={`/${workspaceSlug}`}
        onClick={(event) => {
          if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
          event.preventDefault();
          onBack();
        }}
        className="text-caption inline-flex w-fit items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
      >
        <ArrowLeft size={14} strokeWidth={1.5} aria-hidden="true" />
        Back to workspace
      </a>
      <div className="flex min-w-0 items-center gap-(--space-2)">
        <WorkspaceLogo src={logoUrl} name={workspaceName} />
        <div className="flex min-w-0 flex-col">
          <span className="text-body truncate font-medium text-(--text)">{workspaceName}</span>
          <span className="text-caption text-(--text-muted)">{roleLabel}</span>
        </div>
      </div>
    </div>
  );
}
