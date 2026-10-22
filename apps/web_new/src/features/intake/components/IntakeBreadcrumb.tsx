// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake trail (parity/specs/intake.md INT-001): workspace, project,
// then the Intake crumb linking back to the canonical address. The crumb
// is the terminal entry and stays plain text with aria-current while the
// screen is the destination; its trailing link is the intake address
// itself so the gate driver can read the target.

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
      <Breadcrumbs entries={[{ label: workspaceName }, { label: projectName }, { label: INTAKE_AREA_LABEL }]} />
      <a href={href} className="sr-only">
        {INTAKE_AREA_LABEL}
      </a>
    </div>
  );
}
