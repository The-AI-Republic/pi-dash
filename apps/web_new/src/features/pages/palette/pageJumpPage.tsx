// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages jump sub-page (parity/specs/pages.md §13.6, PAGE-090). The palette
// lists every page the caller can see across the whole tree and navigates
// to the picked one; each option carries its parent path, so a child found
// by name still says where it lives. The rows come from the same forest
// read the list's discovery pass uses, keeping one cache entry for both.

import { useQuery } from "@tanstack/react-query";

import { getAppClient } from "../../../core/api/client.js";
import {
  filterPageOptions,
  useCommandPalette,
  type PalettePage,
  type PalettePageState,
} from "../../../shared/commands/index.js";
import { pageForestQueryOptions } from "../api/treeQueries.js";
import { pageDisplayTitle } from "../lib/pageList.js";
import { pageParentPath } from "../lib/pageNested.js";
import { PAGE_JUMP_PAGE_ID, PAGE_JUMP_TITLE } from "./pageJumpCommands.js";

/** Input prompt while the jump page is open; matches the command title. */
export const PAGE_JUMP_PLACEHOLDER = PAGE_JUMP_TITLE;
/** Plain line while the project has no pages to open. */
export const PAGE_JUMP_EMPTY_TEXT = "No pages yet";

export interface PageJumpPageArgs {
  workspaceSlug: string;
  projectId: string;
  /** Navigate to the picked page's detail. */
  onPick: (pageId: string) => void;
}

export function projectPagesJumpPage(args: PageJumpPageArgs): PalettePage {
  return {
    id: PAGE_JUMP_PAGE_ID,
    placeholder: PAGE_JUMP_PLACEHOLDER,
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      const forest = useQuery(pageForestQueryOptions(getAppClient(), args.workspaceSlug, args.projectId, true));
      if (forest.isPending) {
        return { status: "loading", options: [], emptyText: PAGE_JUMP_EMPTY_TEXT };
      }
      if (forest.isError) {
        return { status: "ready", options: [], emptyText: PAGE_JUMP_EMPTY_TEXT };
      }
      // The palette spans the archived tab too: a shelved child is still a
      // page the caller can open, and PAGE-090 asks every discovery surface
      // to find child pages with their parent path.
      const rows = forest.data.rows;
      const options = rows.map((row) => {
        const path = pageParentPath(rows, row.id);
        return {
          id: row.id,
          title: pageDisplayTitle(row.name),
          ...(path === null ? {} : { hint: path }),
          run: () => {
            closePalette();
            args.onPick(row.id);
          },
        };
      });
      return { status: "ready", options: filterPageOptions(options, query), emptyText: PAGE_JUMP_EMPTY_TEXT };
    },
  };
}
