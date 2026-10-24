// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module work-item entry rules (parity/specs/modules.md §8.3, MOD-031).
// The detail page adds work items in three places — the header and the two
// empty cards — and all of them answer to the same standing rule: a
// completed module reads as a record and a guest (or an archived project)
// never offers the entries. The notices and the picker's search hint are
// pinned by the spec, so they live here instead of at each call site.

import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { isCompletedModule } from "../../../shared/pickers/modules.js";

/** Header and scoped-creation entry. */
export const MODULE_ADD_WORK_ITEM_LABEL = "Add work item";

/** Pinned success notice after existing work items join the module. */
export const MODULE_ADD_EXISTING_SUCCESS = "Work items added to the module successfully.";

/** Pinned failure notice; nothing was attached. */
export const MODULE_ADD_EXISTING_FAILURE = "Selected work items could not be added to the module.";

/** The picker searches as you type; the hint names that behavior. */
export const MODULE_PICKER_SEARCH_HINT = "Type to search";

export interface ModuleAddStanding {
  /** The viewer's project membership role, as the project row reports it. */
  memberRole: number | null;
  /** True while the project itself is archived. */
  archived: boolean;
  /** The module's stored status. */
  moduleStatus: string | undefined;
}

/**
 * Whether the page offers any add entry. Admins and members of a live
 * project may add; a completed module, an archived project and every
 * guest standing read false, which also disables the empty-card actions.
 */
export function canAddWorkItemsToModule({ memberRole, archived, moduleStatus }: ModuleAddStanding): boolean {
  return canEditInProject({ memberRole, archived }) && !isCompletedModule(moduleStatus);
}
