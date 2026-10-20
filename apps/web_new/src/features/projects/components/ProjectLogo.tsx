// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project logo box (SHELL-032, PSET-005/007). An uploaded image, an emoji
// glyph, a chosen symbol in its colour, otherwise the short code's initial. Emoji marks are read through
// the shared decoder, so a stored decimal code-point value renders like the
// literal character. Decorative: the card name already names the project
// for assistive tech.
import * as React from "react";

import { projectIconGlyph, projectMark, projectMarkText, resolveMarkSource } from "../../../shared/pickers/index.js";

export interface ProjectLogoProps {
  /** Raw logo value from the project row; parsed defensively. */
  logoProps: Record<string, unknown>;
  /** Short code; its initial is the fallback mark. */
  identifier: string;
  /** Base URL that relative uploaded-emblem paths resolve against. */
  apiBaseUrl?: string | undefined;
}

/** The picked emoji glyph, or null when none is set. */
export function selectLogoGlyph(logoProps: Record<string, unknown>): string | null {
  const mark = projectMark(logoProps);
  return mark.kind === "emoji" ? mark.glyph : null;
}

/** The uploaded emblem's URL, or null when the mark is not an image. */
export function selectLogoImage(logoProps: Record<string, unknown>): string | null {
  const mark = projectMark(logoProps);
  return mark.kind === "image" ? mark.url : null;
}

export function ProjectLogo({ logoProps, identifier, apiBaseUrl }: ProjectLogoProps): React.ReactElement {
  const mark = projectMark(logoProps);
  const image = mark.kind === "image" ? resolveMarkSource(mark.url, apiBaseUrl) : null;
  const Icon = mark.kind === "icon" ? projectIconGlyph(mark.name) : null;
  return (
    <span
      aria-hidden="true"
      data-testid="project-logo"
      data-mark={mark.kind}
      className="text-h3 flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-(--radius-control) bg-(--subtle)"
    >
      {image !== null ? (
        <img key={image} src={image} alt="" className="size-full object-cover" data-testid="project-logo-image" />
      ) : Icon !== null && mark.kind === "icon" ? (
        <Icon
          size={22}
          strokeWidth={1.5}
          aria-hidden="true"
          data-testid="project-logo-icon"
          {...(mark.color === null ? {} : { color: mark.color, style: { color: mark.color } })}
        />
      ) : (
        projectMarkText(mark, identifier)
      )}
    </span>
  );
}
