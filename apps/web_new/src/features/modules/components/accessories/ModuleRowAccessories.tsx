// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The list row's inline accessory group (parity/specs/modules.md §4,
// MOD-013–016). Slice D's row hands this group its trailing slot: the
// date control, the status pill, the lead mark and — for editors on
// live modules only — the favorite star. Standing comes from the one
// permission helper, never from a role read here: guests and archived
// rows get the date and status controls disabled and no star at all.

import type { ModuleRow, WorkspaceMember } from "@pidash/api-client";
import * as React from "react";

import { resolveModuleLead } from "../../lib/moduleIdentity.js";
import { canEditModule, canShowModuleStar } from "../../lib/modulePermissions.js";
import { ModuleDateControl } from "./ModuleDateControl.js";
import { ModuleLeadMark } from "./ModuleLeadMark.js";
import { ModuleStarButton } from "./ModuleStarButton.js";
import { ModuleStatusControl } from "./ModuleStatusControl.js";

export interface ModuleRowAccessoriesProps {
  workspaceSlug: string;
  projectId: string;
  module: ModuleRow;
  /** The viewer's project role number, null when outside. */
  memberRole: number | null;
  /** True while the row renders in archived mode. */
  archived: boolean;
  /** The workspace directory the lead mark resolves against. */
  members: readonly WorkspaceMember[] | undefined;
}

export function ModuleRowAccessories({
  workspaceSlug,
  projectId,
  module,
  memberRole,
  archived,
  members,
}: ModuleRowAccessoriesProps): React.ReactElement {
  const editable = canEditModule({ memberRole, archived });
  const showStar = canShowModuleStar({ memberRole, archived });
  const lead = resolveModuleLead(module, members);
  return (
    <div data-testid="modules-row-accessories" className="flex shrink-0 items-center gap-(--space-2)">
      <ModuleDateControl
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        module={module}
        disabled={!editable}
        testId="modules-row-dates"
      />
      <ModuleStatusControl
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        module={module}
        disabled={!editable}
        testId="modules-row-status"
      />
      <ModuleLeadMark lead={lead.kind === "avatar" ? lead.person : null} testId="modules-row-lead" />
      {showStar ? (
        <ModuleStarButton
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          module={module}
          testId="modules-row-star"
        />
      ) : null}
    </div>
  );
}
