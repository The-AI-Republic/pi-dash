/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import { observer } from "mobx-react";
import { WorkspaceAppSidebar } from "@/components/workspace/sidebar/app/workspace-app-sidebar";

type Props = {
  children: ReactNode;
  extendedSidebar?: ReactNode;
  includePortal?: boolean;
};

/** Shared shell for workspace routes that mount the persistent app sidebar. */
export const WorkspaceShell = observer(function WorkspaceShell({
  children,
  extendedSidebar,
  includePortal = false,
}: Props) {
  return (
    <div className="relative flex h-full w-full flex-col overflow-hidden rounded-lg border border-subtle">
      {includePortal && <div id="full-screen-portal" className="absolute inset-0 w-full" />}
      <div className="relative flex size-full overflow-hidden">
        <WorkspaceAppSidebar />
        {extendedSidebar}
        <main className="relative flex h-full w-full flex-col overflow-hidden bg-surface-1">{children}</main>
      </div>
    </div>
  );
});
