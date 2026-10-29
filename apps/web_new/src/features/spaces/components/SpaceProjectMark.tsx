// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Navbar project mark (parity/specs/space.md SPC-010). Four branches,
// in order: a configured icon renders its Lucide glyph in its stored
// color through the shared lazy icon wrapper; a configured emoji
// renders its decoded glyph; a project with no configured mark renders
// an empty slot — the old board invents no mark here and neither does
// the new one; and missing project details altogether render a generic
// mark so the bar stays intact. A logo_props value that is not a map
// takes the same generic branch, but a corrupt emoji code point is a
// payload fault and throws to the space error boundary (SPC-019). The
// slot always carries data-mark naming its branch for the area gate's
// driver, which reads the branch rather than guessing from pixels —
// including while the icon chunk is still loading and for icon names
// the glyph set does not know, where the shared wrapper shows the
// identifier initial.
import { Layers } from "lucide-react";
import type { SpaceBoardProject } from "@pidash/api-client";
import * as React from "react";

import { projectMark } from "../../../shared/pickers/projectIcon.js";
import { ProjectMarkIcon } from "../../../shared/pickers/ProjectMarkIcon.js";
import { projectMarkGlyph } from "../lib/projectMark.js";

/** The mark branch a navbar slot settled on. */
export type SpaceMarkKind = "icon" | "emoji" | "empty" | "fallback";

type ResolvedSpaceMark =
  | { kind: "icon"; name: string; color: string | null; identifier: string }
  | { kind: "emoji"; glyph: string }
  | { kind: "empty" }
  | { kind: "fallback" };

/** The logo map when it is one; anything else is a malformed mark. */
function asLogoMap(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/**
 * The decoded emoji glyph, or null when no emoji is configured. A corrupt
 * code point throws to the space error boundary (SPC-019) rather than
 * rendering a fallback.
 */
function safeEmojiGlyph(logoProps: Record<string, unknown>): string | null {
  return projectMarkGlyph(logoProps);
}

/** Settle the mark branch and the payload its render needs. */
function resolveMark(details: SpaceBoardProject | null): ResolvedSpaceMark {
  if (details === null) return { kind: "fallback" };
  const logoProps = asLogoMap(details.logo_props);
  if (logoProps === null) return { kind: "fallback" };
  const mark = projectMark(logoProps);
  if (mark.kind === "icon") return { kind: "icon", name: mark.name, color: mark.color, identifier: details.identifier };
  if (mark.kind === "emoji") {
    const glyph = safeEmojiGlyph(logoProps);
    return glyph === null ? { kind: "fallback" } : { kind: "emoji", glyph };
  }
  return { kind: "empty" };
}

/** Which branch the navbar mark takes for these project details. */
export function resolveSpaceMarkKind(details: SpaceBoardProject | null): SpaceMarkKind {
  return resolveMark(details).kind;
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
  const resolved = resolveMark(details);
  if (resolved.kind === "fallback") {
    return (
      <MarkSlot kind="fallback">
        <Layers aria-hidden="true" size={16} strokeWidth={1.5} />
      </MarkSlot>
    );
  }
  if (resolved.kind === "icon") {
    return (
      <MarkSlot kind="icon">
        <ProjectMarkIcon
          mark={{ kind: "icon", name: resolved.name, color: resolved.color }}
          identifier={resolved.identifier}
          size={16}
          testId="space-navbar-mark-glyph"
        />
      </MarkSlot>
    );
  }
  if (resolved.kind === "emoji") {
    return <MarkSlot kind="emoji">{resolved.glyph}</MarkSlot>;
  }
  return <MarkSlot kind="empty" />;
}
