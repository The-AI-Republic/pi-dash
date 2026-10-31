// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module contextual palette entries (shell spec, SHELL-091). Pure
// selector: the member picker (stays open across toggles), the status
// picker, the favorite toggle and copy-URL, all gated on project
// standing (admin or member, not archived). Writes go through the
// thin module contracts; full module ownership stays in the modules
// area.

import { MODULE_STATUSES, type ModuleStatus } from "@pidash/api-client";

import type { PalettePage, PalettePageOption, PalettePageState, PidashCommand } from "../../commands/registry.js";
import type { NamedProjectMember } from "./api.js";
import { canEditInProject, CONTEXT_INDICATORS, CONTEXT_SECTIONS, type ProjectStanding } from "./types.js";

const STATUS_TITLES: Record<ModuleStatus, string> = {
  backlog: "Backlog",
  planned: "Planned",
  "in-progress": "In Progress",
  paused: "Paused",
  completed: "Completed",
  cancelled: "Cancelled",
};

export interface ModuleContextInput {
  moduleId: string;
  /** Module name shown in the chip. */
  moduleName: string;
  archived: boolean;
  status: ModuleStatus;
  memberIds: string[];
  favorite: boolean;
  standing: ProjectStanding;
  members: NamedProjectMember[];
  membersLoading: boolean;
  toggleMember: (userId: string, member: boolean) => void;
  setStatus: (status: ModuleStatus) => void;
  toggleFavorite: () => void;
  copyUrl: () => void;
  /**
   * Close the palette. The single-change status picker calls this on
   * every pick; the member picker stays open across toggles.
   */
  close: () => void;
}

export interface ModuleContextSelection {
  commands: PidashCommand[];
  pages: PalettePage[];
}

function filterOptions(options: PalettePageOption[], query: string): PalettePageOption[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return options;
  return options.filter((option) => option.title.toLowerCase().includes(needle));
}

function readyState(options: PalettePageOption[], emptyText: string, loading: boolean) {
  return (query: string): PalettePageState => {
    if (loading) return { status: "loading", options: [], emptyText };
    return { status: "ready", options: filterOptions(options, query), emptyText };
  };
}

export function selectModuleContext(input: ModuleContextInput): ModuleContextSelection {
  const section = CONTEXT_SECTIONS.module;
  const context = { kind: "module" as const, indicator: CONTEXT_INDICATORS.module, subject: input.moduleName };
  const prefix = `context.module.${input.moduleId}`;
  const commands: PidashCommand[] = [];
  const pages: PalettePage[] = [];

  if (canEditInProject(input.standing) && !input.archived) {
    commands.push({
      id: `${prefix}.members`,
      title: "Add/remove members",
      section,
      shortcut: "m",
      page: `${prefix}.members`,
      context,
      run: () => undefined,
    });
    pages.push({
      id: `${prefix}.members`,
      placeholder: "Change members",
      usePageState: readyState(
        input.members.map((member) => ({
          id: member.userId,
          title: member.displayName,
          selected: input.memberIds.includes(member.userId),
          run: () => input.toggleMember(member.userId, input.memberIds.includes(member.userId)),
        })),
        "No members found",
        input.membersLoading
      ),
    });

    commands.push({
      id: `${prefix}.status`,
      title: "Change status",
      section,
      shortcut: "s",
      page: `${prefix}.status`,
      context,
      run: () => undefined,
    });
    pages.push({
      id: `${prefix}.status`,
      placeholder: "Change status",
      usePageState: readyState(
        MODULE_STATUSES.map((status) => ({
          id: status,
          title: STATUS_TITLES[status],
          selected: status === input.status,
          run: () => {
            if (status !== input.status) input.setStatus(status);
            input.close();
          },
        })),
        "No statuses found",
        false
      ),
    });

    commands.push({
      id: `${prefix}.favorite`,
      title: input.favorite ? "Remove from favorites" : "Add to favorites",
      section,
      shortcut: "shift+f",
      context,
      run: () => input.toggleFavorite(),
    });
  }

  commands.push({
    id: `${prefix}.copy-url`,
    title: "Copy URL",
    section,
    shortcut: "mod+shift+,",
    context,
    run: () => input.copyUrl(),
  });
  return { commands, pages };
}
