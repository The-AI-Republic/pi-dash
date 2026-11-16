// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Lazy border around the Lucide set behind stored project icons
// (parity/specs/issues.md §17, ISS-211; PSET-005/007). Project marks are
// drawn in the shell rail, the page header, cards and pickers, all of which
// are eager on the first paint; the glyph map is not. This wrapper is eager
// but tiny, and fetches the map on its own chunk the first time a mark is
// actually an icon. Until the chunk arrives the tile shows the short code's
// initial, so the box never renders empty.
import * as React from "react";

import { projectMarkText, type ProjectMark } from "./projectIcon.js";

type GlyphModule = typeof import("./projectIconGlyphs.js");

export interface ProjectMarkIconProps {
  /** Decoded icon mark. */
  mark: Extract<ProjectMark, { kind: "icon" }>;
  /** Short code; its initial stands in until the glyph chunk arrives. */
  identifier: string;
  /** Glyph edge in pixels. */
  size: number;
  testId?: string | undefined;
  /** Drawn in place of the initial when the stored name has no glyph. */
  fallback?: React.ReactElement;
}

export function ProjectMarkIcon({
  mark,
  identifier,
  size,
  testId,
  fallback,
}: ProjectMarkIconProps): React.ReactElement {
  const [module, setModule] = React.useState<GlyphModule | null>(null);
  React.useEffect(() => {
    let live = true;
    void import("./projectIconGlyphs.js").then((next) => {
      if (live) setModule(next);
    });
    return () => {
      live = false;
    };
  }, []);
  const initial = fallback ?? <span aria-hidden="true">{projectMarkText(mark, identifier)}</span>;
  if (module === null) return initial;
  const Glyph = module.projectIconGlyph(mark.name);
  if (Glyph === null) return initial;
  return (
    <Glyph
      size={size}
      strokeWidth={1.5}
      aria-hidden="true"
      data-testid={testId}
      {...(mark.color === null ? {} : { color: mark.color, style: { color: mark.color } })}
    />
  );
}
