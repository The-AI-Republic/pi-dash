// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module work-item entry rules (parity/specs/modules.md §8.3, MOD-031).
// The detail page adds work items in three places — the header and the two
// empty cards — and all of them answer to the same standing rule: an
// admin or member of a live project sees the entries, a guest or an
// archived project does not. Membership, not the module's status, decides
// (the old header gated only on the project role too). The notices and
// the picker's search hint are pinned by the spec, so they live here
// instead of at each call site.

import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import type { CreateProjectOption } from "../../issues/index.js";

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
}

/** The project-list row fields the module page reads for its entries. */
export interface ModuleAddProjectRow {
  id: string;
  name: string;
  identifier: string;
  member_role: number | null;
  archived_at: string | null;
}

/**
 * The list row as the shared creation dialog wants it. The dialog filters
 * on `memberRole`/`archived`, while the API row names those fields
 * `member_role`/`archived_at`; passing the raw row would drop the project.
 */
export function toModuleCreateProject(row: ModuleAddProjectRow): CreateProjectOption {
  return {
    id: row.id,
    name: row.name,
    identifier: row.identifier,
    memberRole: row.member_role,
    archived: row.archived_at !== null,
  };
}

/**
 * Whether the page offers any add entry. Admins and members of a live
 * project may add; an archived project and every guest standing read
 * false, which also disables the empty-card actions.
 */
export function canAddWorkItemsToModule({ memberRole, archived }: ModuleAddStanding): boolean {
  return canEditInProject({ memberRole, archived });
}
