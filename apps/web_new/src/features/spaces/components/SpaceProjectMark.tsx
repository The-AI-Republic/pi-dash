// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navbar project mark (parity/specs/space.md SPC-010). Four branches,
// in order: a configured icon renders its Lucide glyph in its stored
// color through the shared lazy icon wrapper; a configured emoji
// renders its decoded glyph; a project with no configured mark renders
// an empty slot — the old board invents no mark here and neither does
// the new one; and missing project details altogether render a generic
// mark so the bar stays intact. The slot always carries data-mark
// naming its branch for the area gate's driver, which reads the branch
// rather than guessing from pixels — including while the icon chunk
// is still loading and for icon names the glyph set does not know,
// where the shared wrapper shows the identifier initial.
import { Layers } from "lucide-react";
import type { SpaceBoardProject } from "@pidash/api-client";
import * as React from "react";

import { projectMark } from "../../../shared/pickers/projectIcon.js";
import { ProjectMarkIcon } from "../../../shared/pickers/ProjectMarkIcon.js";
import { projectMarkGlyph } from "../lib/projectMark.js";

/** The mark branch a navbar slot settled on. */
export type SpaceMarkKind = "icon" | "emoji" | "empty" | "fallback";

/** Which branch the navbar mark takes for these project details. */
export function resolveSpaceMarkKind(details: SpaceBoardProject | null): SpaceMarkKind {
  if (details === null) return "fallback";
  const mark = projectMark(details.logo_props);
  if (mark.kind === "icon") return "icon";
  if (mark.kind === "emoji") return "emoji";
  return "empty";
}

const SLOT_CLASSES = "text-body flex size-7 items-center justify-center rounded-(--radius-control) bg-(--subtle)";

function MarkSlot({ kind, children }: { kind: SpaceMarkKind; children?: React.ReactNode }): React.ReactElement {
  return (
    <span data-testid="space-navbar-mark" data-mark={kind} className={SLOT_CLASSES}>
      {children}
    </span>
  );
}

/** The project's navbar mark. */
export function SpaceProjectMark({ details }: { details: SpaceBoardProject | null }): React.ReactElement {
  if (details === null) {
    return (
      <MarkSlot kind="fallback">
        <Layers aria-hidden="true" size={16} strokeWidth={1.5} />
      </MarkSlot>
    );
  }
  const mark = projectMark(details.logo_props);
  if (mark.kind === "icon") {
    return (
      <MarkSlot kind="icon">
        <ProjectMarkIcon mark={mark} identifier={details.identifier} size={16} testId="space-navbar-mark-glyph" />
      </MarkSlot>
    );
  }
  if (mark.kind === "emoji") {
    return <MarkSlot kind="emoji">{projectMarkGlyph(details.logo_props)}</MarkSlot>;
  }
  return <MarkSlot kind="empty" />;
}
