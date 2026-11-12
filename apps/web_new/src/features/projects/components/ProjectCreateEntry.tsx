// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Deferred project-creation entry (shell spec, SHELL-086). The workspace
// layout loads this through React.lazy on first open, so the creation
// dialog and its member-directory read ride a deferred chunk instead of
// the initial shell chain (Quality gates budget).

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { workspaceMembersQueryOptions } from "../api/members.js";
import { CreateProjectDialog } from "./CreateProjectDialog.js";

export interface ProjectCreateEntryProps {
  workspaceSlug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ProjectCreateEntry({ workspaceSlug, open, onOpenChange }: ProjectCreateEntryProps): React.ReactElement {
  const memberships = useQuery(workspaceMembersQueryOptions(getAppClient(), workspaceSlug));
  return (
    <CreateProjectDialog
      workspaceSlug={workspaceSlug}
      members={memberships.data}
      open={open}
      onOpenChange={onOpenChange}
    />
  );
}
