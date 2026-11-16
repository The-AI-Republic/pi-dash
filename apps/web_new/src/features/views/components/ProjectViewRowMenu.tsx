// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overflow menu on a project view row (parity/specs/views.md §6,
// VIEW-017, VIEW-020). The container decides which entries the
// caller's standing permits; this menu renders them in order, each
// with an icon, and reports the pick.

import { IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { Ellipsis, ExternalLink, Globe, Link2, Pencil, Trash2 } from "lucide-react";
import * as React from "react";

import type { ProjectViewMenuItemId } from "../lib/projectViewActions.js";

const MENU_COPY: Record<ProjectViewMenuItemId, { label: string; icon: React.ReactNode }> = {
  edit: { label: "Edit", icon: <Pencil size={16} strokeWidth={1.5} aria-hidden="true" /> },
  open: { label: "Open in new tab", icon: <ExternalLink size={16} strokeWidth={1.5} aria-hidden="true" /> },
  copy: { label: "Copy link", icon: <Link2 size={16} strokeWidth={1.5} aria-hidden="true" /> },
  publish: { label: "Publish", icon: <Globe size={16} strokeWidth={1.5} aria-hidden="true" /> },
  delete: { label: "Delete", icon: <Trash2 size={16} strokeWidth={1.5} aria-hidden="true" /> },
};

export interface ProjectViewRowMenuProps {
  viewName: string;
  /** Entries the caller's standing permits, in display order. */
  items: readonly ProjectViewMenuItemId[];
  onSelect: (item: ProjectViewMenuItemId) => void;
}

export function ProjectViewRowMenu({ viewName, items, onSelect }: ProjectViewRowMenuProps): React.ReactElement {
  const entries: MenuEntry[] = items.map((id) => ({
    label: MENU_COPY[id].label,
    itemKey: id,
    icon: MENU_COPY[id].icon,
    onSelect: () => onSelect(id),
  }));
  return (
    <Menu
      trigger={
        <IconButton label={`Actions for ${viewName}`} size="small" data-testid="project-views-row-menu-trigger">
          <Ellipsis size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      }
      items={entries}
    />
  );
}
