// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project logo box (SHELL-032, PSET-005/007). An uploaded image, an emoji
// glyph, a chosen symbol in its colour, otherwise the short code's initial.
// Emoji marks are read through the shared decoder, so a stored decimal
// code-point value renders like the literal character. Decorative: the
// card name already names the project for assistive tech.
import * as React from "react";

import { projectMark, projectMarkText } from "../../../shared/pickers/projectIcon.js";
import { EmblemArtwork } from "./projectLogoImage.js";

export interface ProjectLogoProps {
  /** Raw logo value from the project row; parsed defensively. */
  logoProps: Record<string, unknown>;
  /** Short code; its initial is the fallback mark. */
  identifier: string;
}

/** The picked emoji glyph, or null when none is set. */
export function selectLogoGlyph(logoProps: Record<string, unknown>): string | null {
  const mark = projectMark(logoProps);
  return mark.kind === "emoji" ? mark.glyph : null;
}

export function ProjectLogo({ logoProps, identifier }: ProjectLogoProps): React.ReactElement {
  const mark = projectMark(logoProps);
  return (
    <span
      aria-hidden="true"
      data-testid="project-logo"
      data-mark={mark.kind}
      className="text-h3 flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-(--radius-control) bg-(--subtle)"
    >
      {mark.kind === "image" || mark.kind === "icon" ? (
        <EmblemArtwork mark={mark} identifier={identifier} />
      ) : (
        projectMarkText(mark, identifier)
      )}
    </span>
  );
}
