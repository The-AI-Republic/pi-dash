// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake trail (parity/specs/intake.md INT-001): workspace, project,
// then the Intake crumb. The terminal crumb is the visible link back to
// the canonical intake address, so the trail doubles as a way back to
// the list from a detail selection.

import { Breadcrumbs } from "@pidash/kit";
import * as React from "react";

import { INTAKE_AREA_LABEL } from "../lib/title.js";

export interface IntakeBreadcrumbProps {
  workspaceName: string;
  projectName: string;
  href: string;
}

export function IntakeBreadcrumb({ workspaceName, projectName, href }: IntakeBreadcrumbProps): React.ReactElement {
  return (
    <div data-testid="intake-breadcrumb" className="min-w-0">
      <Breadcrumbs
        terminalAsLink
        entries={[{ label: workspaceName }, { label: projectName }, { label: INTAKE_AREA_LABEL, href }]}
      />
    </div>
  );
}
