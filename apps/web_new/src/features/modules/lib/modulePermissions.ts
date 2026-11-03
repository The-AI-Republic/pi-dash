// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module standing rules (parity/specs/modules.md §4, MOD-013–018). Every
// inline accessory (dates, status, lead affordances, star) and every
// later slice's menu and dialog entry asks these predicates instead of
// reading a role number: admins and members of a live module may act,
// guests and archived modules may not. The role math itself belongs to
// the shared project standing helper; this module only pins what the
// modules area feeds it (the viewer's project role and the row's
// archived mode).

import { canEditInProject } from "../../../shared/shell/contextual/index.js";

/** What the modules area feeds the standing rules. */
export interface ModuleStanding {
  /** The viewer's project role number (20 admin, 15 member, 5 guest), null when outside. */
  memberRole: number | null;
  /** True while the row renders in archived mode (slice G's screen). */
  archived: boolean;
}

/**
 * Whether the viewer may edit this module inline: a project admin or
 * member on a live module. Guests and archived rows read false, which
 * renders the date and status controls disabled.
 */
export function canEditModule({ memberRole, archived }: ModuleStanding): boolean {
  return canEditInProject({ memberRole, archived });
}

/**
 * Whether the row offers the favorite star. Guests see no star at all
 * and archived modules cannot be favorited (archiving strips favorites
 * server-side, so there is no state a star could show); only an editor
 * on a live module sees one.
 */
export function canShowModuleStar({ memberRole, archived }: ModuleStanding): boolean {
  return canEditInProject({ memberRole, archived });
}
