// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project logo box (SHELL-032). An emoji glyph when the project picked
// one, otherwise the short code's initial. Decorative: the card name
// already names the project for assistive tech.
import * as React from "react";

export interface ProjectLogoProps {
  /** Raw logo value from the project row; parsed defensively. */
  logoProps: Record<string, unknown>;
  /** Short code; its initial is the fallback mark. */
  identifier: string;
}

/** The picked emoji glyph, or null when none is set. */
export function selectLogoGlyph(logoProps: Record<string, unknown>): string | null {
  if (logoProps["in_use"] !== "emoji") return null;
  const emoji = logoProps["emoji"];
  if (typeof emoji !== "object" || emoji === null) return null;
  const value = (emoji as Record<string, unknown>)["value"];
  if (typeof value !== "string" || value.trim() === "") return null;
  return value;
}

export function ProjectLogo({ logoProps, identifier }: ProjectLogoProps): React.ReactElement {
  const glyph = selectLogoGlyph(logoProps);
  const initial = identifier.trim().slice(0, 1).toUpperCase();
  return (
    <span
      aria-hidden="true"
      data-testid="project-logo"
      className="text-h3 flex h-10 w-10 shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle)"
    >
      {glyph ?? (initial === "" ? "?" : initial)}
    </span>
  );
}
