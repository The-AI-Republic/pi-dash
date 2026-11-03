// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overflow menu on a custom workspace view row (parity/specs/views.md §9,
// VIEW-032). The menu renders for every caller who can see the row: the
// server, not the client, decides who may edit (owner) or delete (owner or
// workspace admin). Edit and Delete act on the row's record through the
// handlers the workspace CRUD slice supplies.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Ellipsis } from "lucide-react";
import * as React from "react";

export interface WorkspaceViewRowMenuProps {
  viewName: string;
  /** Opens the edit dialog for this view. */
  onEdit: () => void;
  /** Opens the delete confirmation for this view. */
  onDelete: () => void;
}

export function WorkspaceViewRowMenu({ viewName, onEdit, onDelete }: WorkspaceViewRowMenuProps): React.ReactElement {
  const items: MenuEntry[] = [
    { label: "Edit View", itemKey: "edit", onSelect: onEdit },
    { label: "Delete View", itemKey: "delete", onSelect: onDelete },
  ];
  return (
    <Menu
      trigger={
        <IconButton label={`Actions for ${viewName}`} data-testid="workspace-views-row-menu-trigger">
          <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      }
      items={items}
    />
  );
}
