/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Command } from "cmdk";
import { observer } from "mobx-react";
// apple pi dash types
import { useParams } from "next/navigation";
import type { TIssue } from "@apple-pi-dash/types";
import { Spinner } from "@apple-pi-dash/ui";
// hooks
import { useProjectState } from "@/hooks/store/use-project-state";
// local imports
import { PowerKProjectStatesMenuItems } from "@/apple-pi-dash-web/components/command-palette/power-k/pages/context-based/work-item/state-menu-item";

type Props = {
  handleSelect: (stateId: string) => void;
  workItemDetails: TIssue;
};

export const PowerKProjectStatesMenu = observer(function PowerKProjectStatesMenu(props: Props) {
  const { workItemDetails } = props;
  // router
  const { workspaceSlug } = useParams();
  // store hooks
  const { getProjectStateIds, getStateById } = useProjectState();
  // derived values
  const projectStateIds = workItemDetails.project_id ? getProjectStateIds(workItemDetails.project_id) : undefined;
  const projectStates = projectStateIds ? projectStateIds.map((stateId) => getStateById(stateId)) : undefined;
  const filteredProjectStates = projectStates ? projectStates.filter((state) => !!state) : undefined;

  if (!filteredProjectStates) return <Spinner />;

  return (
    <Command.Group>
      <PowerKProjectStatesMenuItems
        {...props}
        projectId={workItemDetails.project_id ?? undefined}
        selectedStateId={workItemDetails.state_id ?? undefined}
        states={filteredProjectStates}
        workspaceSlug={workspaceSlug?.toString()}
      />
    </Command.Group>
  );
});
