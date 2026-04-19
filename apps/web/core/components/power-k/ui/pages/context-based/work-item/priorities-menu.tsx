/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Command } from "cmdk";
import { observer } from "mobx-react";
// pi dash imports
import { ISSUE_PRIORITIES } from "@pi-dash/constants";
import { PriorityIcon } from "@pi-dash/propel/icons";
import type { TIssue, TIssuePriorities } from "@pi-dash/types";
// local imports
import { PowerKModalCommandItem } from "../../../modal/command-item";

type Props = {
  handleSelect: (priority: TIssuePriorities) => void;
  workItemDetails: TIssue;
};

export const PowerKWorkItemPrioritiesMenu = observer(function PowerKWorkItemPrioritiesMenu(props: Props) {
  const { handleSelect, workItemDetails } = props;

  return (
    <Command.Group>
      {ISSUE_PRIORITIES.map((priority) => (
        <PowerKModalCommandItem
          key={priority.key}
          iconNode={<PriorityIcon priority={priority.key} />}
          label={priority.title}
          isSelected={priority.key === workItemDetails.priority}
          onSelect={() => handleSelect(priority.key)}
        />
      ))}
    </Command.Group>
  );
});
