/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
import { observer } from "mobx-react";
import { ProjectAppSidebar } from "@/app/(all)/[workspaceSlug]/(projects)/_sidebar";

type Props = {
  children: ReactNode;
  extendedSidebar?: ReactNode;
};

export const WorkspaceShell = observer(function WorkspaceShell({ children, extendedSidebar }: Props) {
  return (
    <div className="relative flex h-full w-full flex-col overflow-hidden rounded-lg border border-subtle">
      <div className="relative flex size-full overflow-hidden">
        <ProjectAppSidebar />
        {extendedSidebar}
        <main className="relative flex h-full w-full flex-col overflow-hidden bg-surface-1">{children}</main>
      </div>
    </div>
  );
});
