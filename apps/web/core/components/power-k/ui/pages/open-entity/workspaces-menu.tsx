/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
// apple pi dash types
import type { IWorkspace } from "@apple-pi-dash/types";
import { Spinner } from "@apple-pi-dash/ui";
// components
import { PowerKWorkspacesMenu } from "@/components/power-k/menus/workspaces";
// hooks
import { useWorkspace } from "@/hooks/store/use-workspace";

type Props = {
  handleSelect: (workspace: IWorkspace) => void;
};

export const PowerKOpenWorkspaceMenu = observer(function PowerKOpenWorkspaceMenu(props: Props) {
  const { handleSelect } = props;
  // store hooks
  const { loader, workspaces } = useWorkspace();
  // derived values
  const workspacesList = workspaces ? Object.values(workspaces) : [];

  if (loader) return <Spinner />;

  return <PowerKWorkspacesMenu workspaces={workspacesList} onSelect={handleSelect} />;
});
