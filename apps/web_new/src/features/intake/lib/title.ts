// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake document title (parity/specs/intake.md INT-003): the project
// name plus the area label, with the product name standing in until the
// project row resolves. Pure.

import { PRODUCT_NAME } from "../../instance/index.js";

/** Breadcrumb label of the intake area. */
export const INTAKE_AREA_LABEL = "Intake";

/** `"<project> - Intake"`, or `"<product> - Intake"` before the name is known. */
export function intakeDocumentTitle(projectName: string | null | undefined): string {
  const name =
    projectName === null || projectName === undefined || projectName.trim() === "" ? PRODUCT_NAME : projectName;
  return `${name} - ${INTAKE_AREA_LABEL}`;
}
