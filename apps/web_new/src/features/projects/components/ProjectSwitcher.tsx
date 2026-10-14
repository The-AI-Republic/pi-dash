// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project switcher (parity/specs/shell.md §7, SHELL-072/073). The header
// button names the current project, truncates long names with the full
// text on hover, and opens the joined-projects dropdown.

import { Avatar, Menu, Tooltip } from "@pidash/kit";
import { ChevronDown } from "lucide-react";
import * as React from "react";

export interface SwitcherProject {
  id: string;
  name: string;
}

export interface ProjectSwitcherProps {
  current: SwitcherProject;
  joined: SwitcherProject[];
  onSelectProject: (projectId: string) => void;
}

export function ProjectSwitcher({ current, joined, onSelectProject }: ProjectSwitcherProps): React.ReactElement {
  return (
    <Tooltip content={current.name}>
      <span className="flex min-w-0 items-center">
        <Menu
          trigger={
            <button
              type="button"
              data-testid="project-switcher-trigger"
              className="flex h-9 min-w-0 cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) pr-(--space-2) pl-(--space-1) hover:bg-(--subtle)"
            >
              <span aria-hidden="true" className="inline-flex shrink-0 items-center">
                <Avatar name={current.name} size="small" />
              </span>
              <span
                data-testid="project-switcher-name"
                className="text-emphasis max-w-44 min-w-0 flex-1 truncate text-left text-(--text)"
              >
                {current.name}
              </span>
              <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
            </button>
          }
          items={joined.map((project) => ({
            label: project.name,
            itemKey: project.id,
            icon: <Avatar name={project.name} size="small" />,
            onSelect: () => onSelectProject(project.id),
          }))}
        />
      </span>
    </Tooltip>
  );
}
