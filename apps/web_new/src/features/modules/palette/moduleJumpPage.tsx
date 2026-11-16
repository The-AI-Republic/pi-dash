// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module jump sub-page (parity/specs/modules.md §10.2, MOD-037). Lists the
// project's non-archived modules, each with the status dot the modules
// list uses, and navigates to the picked module. It is a page over one
// list read, never one command per module, and it shows no selection
// marks: choosing a row goes somewhere, it does not toggle anything. A
// failed read reads as an empty page (SHELL-093), not an error screen.

import { useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import {
  filterPageOptions,
  useCommandPalette,
  type PalettePage,
  type PalettePageOption,
  type PalettePageState,
} from "../../../shared/commands/index.js";
import { modulesListQueryOptions } from "../api/queries.js";
import { moduleStatusColor, moduleStatusLabel } from "../lib/moduleListControls.js";
import { MODULE_JUMP_PAGE_ID, MODULE_JUMP_TITLE } from "./moduleCommands.js";
import { ModuleStatusDot } from "./ModuleStatusDot.js";

/** Input prompt while the jump page is open; matches the command title. */
export const MODULE_JUMP_PLACEHOLDER = MODULE_JUMP_TITLE;
/** Plain line while the project has no modules to open. */
export const MODULE_JUMP_EMPTY_TEXT = "No modules yet";

export interface ModuleJumpPageArgs {
  workspaceSlug: string;
  projectId: string;
  /** Navigate to the picked module's detail page. */
  onPick: (moduleId: string) => void;
}

export function projectModulesJumpPage(args: ModuleJumpPageArgs): PalettePage {
  return {
    id: MODULE_JUMP_PAGE_ID,
    placeholder: MODULE_JUMP_PLACEHOLDER,
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const modules = useQuery(modulesListQueryOptions(getAppClient(), args.workspaceSlug, args.projectId));
      if (modules.isPending) {
        return { status: "loading", options: [], emptyText: MODULE_JUMP_EMPTY_TEXT };
      }
      if (modules.isError) {
        return { status: "ready", options: [], emptyText: MODULE_JUMP_EMPTY_TEXT };
      }
      const options: PalettePageOption[] = modules.data.map((module) => ({
        id: module.id,
        title: module.name,
        hint: moduleStatusLabel(module.status),
        icon: <ModuleStatusDot status={module.status} color={moduleStatusColor(module.status)} />,
        run: () => {
          closePalette();
          args.onPick(module.id);
        },
      }));
      return {
        status: "ready",
        options: filterPageOptions(options, query),
        emptyText: MODULE_JUMP_EMPTY_TEXT,
      };
    },
  };
}
