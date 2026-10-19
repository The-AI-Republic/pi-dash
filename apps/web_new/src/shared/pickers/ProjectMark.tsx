// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mark tile (parity/specs/issues.md §17, ISS-211). An emoji, the
// project's chosen icon in its own color, or the short code's initial, in
// the tile size each caller asks for. Decorative: the row label already
// names the project for assistive tech.
import * as React from "react";

import { projectIconGlyph, projectMark, projectMarkText } from "./projectIcon.js";

export interface ProjectMarkProps {
  /** Raw logo value from the project row; parsed defensively. */
  logoProps: Record<string, unknown>;
  /** Short code; its initial is the fallback mark. */
  identifier: string;
  /** Tile edge in pixels. */
  size?: number;
  className?: string;
}

export function ProjectMark({ logoProps, identifier, size = 20, className }: ProjectMarkProps): React.ReactElement {
  const mark = projectMark(logoProps);
  const Icon = mark.kind === "icon" ? projectIconGlyph(mark.name) : null;
  const fontSize = size <= 18 ? "text-meta" : "text-body";
  return (
    <span
      aria-hidden="true"
      data-testid="project-mark"
      data-mark={mark.kind}
      className={`flex shrink-0 items-center justify-center rounded-(--radius-control) bg-(--subtle) ${fontSize} font-medium text-(--text-muted) ${className ?? ""}`}
      style={{ width: size, height: size }}
    >
      {Icon !== null && mark.kind === "icon" ? (
        <Icon
          size={Math.round(size * 0.7)}
          strokeWidth={1.5}
          {...(mark.color === null ? {} : { color: mark.color, style: { color: mark.color } })}
        />
      ) : (
        projectMarkText(mark, identifier)
      )}
    </span>
  );
}
