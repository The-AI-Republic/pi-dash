// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module row lead mark (parity/specs/modules.md §4.3, MOD-015). A set
// lead shows their avatar; otherwise a neutral person glyph explains on
// hover that the module has no lead. The caller resolves the lead
// against the workspace directory (resolveModuleLead); a lead the
// viewer cannot read arrives as null and reads as no lead. Changing the
// lead is an editor action in the edit dialog and the sidebar, never an
// inline picker here.

import { Avatar, Tooltip } from "@pidash/kit";
import { SquareUser } from "lucide-react";
import * as React from "react";

import type { ModuleLeadPerson } from "../../lib/moduleIdentity.js";

export interface ModuleLeadMarkProps {
  /** The resolved lead, or null for the no-lead mark. */
  lead: ModuleLeadPerson | null;
  /** Test handle carrying the data-lead-kind reading. */
  testId: string;
}

/** The no-lead explanation the tooltip carries (pinned by the oracle). */
export const MODULE_NO_LEAD_TOOLTIP = "No lead";

export function ModuleLeadMark({ lead, testId }: ModuleLeadMarkProps): React.ReactElement {
  if (lead === null) {
    return (
      <Tooltip content={MODULE_NO_LEAD_TOOLTIP}>
        <span data-testid={testId} data-lead-kind="none" className="inline-flex items-center text-(--text-muted)">
          <SquareUser size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="sr-only">{MODULE_NO_LEAD_TOOLTIP}</span>
        </span>
      </Tooltip>
    );
  }
  return (
    <span data-testid={testId} data-lead-kind="avatar" className="inline-flex items-center">
      <Avatar name={lead.name} size="small" {...(lead.avatarUrl === null ? {} : { src: lead.avatarUrl })} />
    </span>
  );
}
