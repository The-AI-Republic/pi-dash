// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-views jump sub-page (parity/specs/views.md §12, VIEW-042).
// Lists the current project's views behind the "Open a project view"
// palette command and navigates on pick. Reads through the same query
// the list screen uses, so both share one cache entry; the palette
// renders the result through its generic page view (spinner, options,
// empty line). A failed read renders as empty with no error UI
// (SHELL-093's rule, reused rather than re-implemented).

import { useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import {
  filterPageOptions,
  useCommandPalette,
  type PalettePage,
  type PalettePageState,
} from "../../../shared/commands/index.js";
import { projectViewsQueryOptions } from "../api/queries.js";
import { sortProjectViewsDefault } from "../lib/projectViews.js";
import { VIEW_JUMP_PAGE_ID, VIEW_JUMP_TITLE } from "./viewCommands.js";

/** Input prompt while the jump page is open; matches the command title. */
export const VIEW_JUMP_PAGE_PLACEHOLDER = VIEW_JUMP_TITLE;
/** Plain line while the project holds no views. */
export const VIEW_JUMP_EMPTY_TEXT = "No project views yet";

export interface ViewJumpPageArgs {
  workspaceSlug: string;
  projectId: string;
  /** Navigate to the picked view's detail. */
  onPick: (viewId: string) => void;
}

export function projectViewsJumpPage(args: ViewJumpPageArgs): PalettePage {
  return {
    id: VIEW_JUMP_PAGE_ID,
    placeholder: VIEW_JUMP_PAGE_PLACEHOLDER,
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const views = useQuery(projectViewsQueryOptions(getAppClient(), args.workspaceSlug, args.projectId));
      if (views.isPending) {
        return { status: "loading", options: [], emptyText: VIEW_JUMP_EMPTY_TEXT };
      }
      if (views.isError) {
        return { status: "ready", options: [], emptyText: VIEW_JUMP_EMPTY_TEXT };
      }
      const ordered = sortProjectViewsDefault(views.data);
      return {
        status: "ready",
        options: filterPageOptions(
          ordered.map((view) => ({
            id: view.id,
            title: view.name,
            run: () => {
              closePalette();
              args.onPick(view.id);
            },
          })),
          query
        ),
        emptyText: VIEW_JUMP_EMPTY_TEXT,
      };
    },
  };
}
