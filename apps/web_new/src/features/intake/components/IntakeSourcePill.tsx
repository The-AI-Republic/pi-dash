// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake source pill (parity/specs/intake.md INT-034). The request's
// origin marker for rows and the detail: cloud-only, behind the
// intake-source-pill edition flag. In-app requests are the bare default
// and never show a pill; any other origin renders its humanized token.

import * as React from "react";

import { edition, INTAKE_SOURCE_PILL_FLAG } from "../../../core/edition/index.js";

/** The bare-default origin: requests filed inside the app. */
export const INTAKE_IN_APP_SOURCE = "IN_APP";

/**
 * Pill text for an origin token, or null for the bare default. Unknown
 * tokens humanize generically (`CONTACT_FORM` reads "Contact Form") so
 * a new cloud origin needs no client change.
 */
export function intakeSourcePillLabel(source: string | null): string | null {
  if (source === null || source === INTAKE_IN_APP_SOURCE) return null;
  const words = source
    .toLowerCase()
    .split("_")
    .filter((word) => word !== "");
  if (words.length === 0) return null;
  return words.map((word) => word[0]!.toUpperCase() + word.slice(1)).join(" ");
}

export interface IntakeSourcePillProps {
  /** The row's origin token (`IN_APP`, null, or a cloud origin). */
  source: string | null;
}

/**
 * The origin pill. Renders nothing unless the edition enables the flag
 * and the origin differs from the in-app default.
 */
export function IntakeSourcePill({ source }: IntakeSourcePillProps): React.ReactElement | null {
  const label = intakeSourcePillLabel(source);
  if (label === null || edition.flags[INTAKE_SOURCE_PILL_FLAG] !== true) return null;
  return (
    <span
      data-testid="intake-source-pill"
      data-source={source}
      className="text-meta rounded-(--radius-full) bg-(--subtle) px-(--space-2) py-px text-(--text-muted)"
    >
      {label}
    </span>
  );
}
