// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General-page identity block (parity/specs/workspace-settings.md §4, §5;
// WSET-004/005): the logo (image or initial fallback), the workspace name,
// the copyable address and the admin's logo action. Members see the block
// dimmed with the copy control still live and no logo action. The dialog
// itself is owned by the caller so the file picker mounts only on open.

import { Button } from "@pidash/kit";
import { Copy, ImageUp, PencilLine } from "lucide-react";
import * as React from "react";

import { workspaceAddress, workspaceInitial } from "../rules.js";

export interface WorkspaceIdentityCardProps {
  workspaceSlug: string;
  workspaceName: string;
  logoSrc: string | null;
  canManage: boolean;
  onOpenLogo: () => void;
  onCopyAddress: () => void;
}

export function WorkspaceIdentityCard({
  workspaceSlug,
  workspaceName,
  logoSrc,
  canManage,
  onOpenLogo,
  onCopyAddress,
}: WorkspaceIdentityCardProps): React.ReactElement {
  const address = workspaceAddress(window.location.origin, workspaceSlug);
  return (
    <div
      data-testid="workspace-general-identity"
      className={canManage ? "flex items-start gap-(--space-4)" : "flex items-start gap-(--space-4) opacity-60"}
    >
      <div
        data-testid="workspace-general-logo"
        className="text-h2 grid size-14 shrink-0 place-items-center overflow-hidden rounded-(--radius-control) bg-(--accent) text-(--on-accent)"
      >
        {logoSrc === null ? (
          <span data-testid="workspace-general-logo-fallback" aria-hidden="true">
            {workspaceInitial(workspaceName)}
          </span>
        ) : (
          <img
            data-testid="workspace-general-logo-image"
            src={logoSrc}
            alt="Workspace Logo"
            className="size-full object-cover"
          />
        )}
      </div>
      <div className="flex min-w-0 flex-col items-start gap-(--space-1)">
        <h2 data-testid="workspace-general-name" className="text-h3 break-words text-(--text)">
          {workspaceName}
        </h2>
        <button
          type="button"
          onClick={onCopyAddress}
          className="text-body flex max-w-full cursor-pointer items-center gap-(--space-1) text-left text-(--accent) hover:text-(--accent-hover)"
        >
          <Copy size={14} strokeWidth={1.5} aria-hidden="true" />
          <span className="truncate">{address}</span>
        </button>
        {canManage ? (
          <Button
            variant="ghost"
            size="small"
            onClick={onOpenLogo}
            className="px-0 text-(--accent) hover:bg-transparent hover:text-(--accent-hover)"
          >
            {logoSrc === null ? (
              <ImageUp size={14} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <PencilLine size={14} strokeWidth={1.5} aria-hidden="true" />
            )}
            {logoSrc === null ? "Upload logo" : "Edit logo"}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
