// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail header menu entries the row actions slice owns
// (parity/specs/modules.md §4.5–§4.6, MOD-018). The detail menu mounts
// the same link actions as rows and cards through the detail actions'
// extra-entries seam, so every role copies and opens from the header;
// slice G appends the Archive entry beside these (MOD-026).

import type { MenuEntry } from "@pidash/kit";

import { copyModuleLink, openModuleInNewTab } from "../../lib/moduleShare.js";
import { MODULE_MENU_COPY, MODULE_MENU_OPEN } from "./moduleMenuModel.js";

/** The detail header's read-only entries: open in a new tab, copy link. */
export function moduleDetailMenuEntries(workspaceSlug: string, projectId: string, moduleId: string): MenuEntry[] {
  return [
    {
      label: MODULE_MENU_OPEN,
      onSelect: () => {
        void openModuleInNewTab(workspaceSlug, projectId, moduleId);
      },
    },
    {
      label: MODULE_MENU_COPY,
      onSelect: () => {
        void copyModuleLink(workspaceSlug, projectId, moduleId);
      },
    },
  ];
}
