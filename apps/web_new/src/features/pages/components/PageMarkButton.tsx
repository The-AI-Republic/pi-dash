// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page mark in the detail canvas (parity/specs/pages.md §5.2, PAGE-027).
// The mark sits above the title: the page's glyph with the picker behind
// it. The picker's affordance fades in when the canvas is hovered or
// something inside it has focus, and stays pinned while the title is
// empty so a brand-new page can find it. Editable pages persist every
// pick, change and clear through the mark mutation; read-only pages show
// the same glyph as static art with no picker at all.

import * as React from "react";

import { ProjectMarkIcon, projectMark } from "../../../shared/pickers/index.js";
import { PageMarkPicker } from "./PageMarkPicker.js";

export const PAGE_MARK_AREA = "page-header-container";
export const PAGE_MARK_CHANGE_LABEL = "Change the page mark";
export const PAGE_MARK_EMPTY_LABEL = "Add a page mark";

/**
 * The mark a detail page carries, in the picker's draft shape. A stored
 * upload mark has no draft equivalent (the picker offers emoji and
 * symbols), so it reads as no mark and the picker starts empty.
 */
export function markDraftOf(logoProps: Record<string, unknown>): Record<string, unknown> {
  const mark = projectMark(logoProps);
  if (mark.kind === "emoji") return { in_use: "emoji", emoji: { value: mark.glyph } };
  if (mark.kind === "icon") {
    return {
      in_use: "icon",
      icon: mark.color === null ? { name: mark.name } : { name: mark.name, color: mark.color },
    };
  }
  return {};
}

/** Trigger name: a page with a mark changes it, one without adds it. */
export function markTriggerLabel(draft: Record<string, unknown>): string {
  return projectMark(draft).kind === "initial" ? PAGE_MARK_EMPTY_LABEL : PAGE_MARK_CHANGE_LABEL;
}

/** Static drawing of a stored mark, used by both faces of the control. */
export function PageMarkGlyph({ logoProps }: { logoProps: Record<string, unknown> }): React.ReactElement {
  const mark = projectMark(logoProps);
  if (mark.kind === "emoji") return <span data-testid="page-mark-glyph">{mark.glyph}</span>;
  if (mark.kind === "icon") {
    return (
      <ProjectMarkIcon
        mark={mark}
        identifier={mark.name}
        size={28}
        testId="page-mark-icon"
        fallback={<span aria-hidden="true">{mark.name.slice(0, 1).toUpperCase()}</span>}
      />
    );
  }
  if (mark.kind === "image") {
    return <img data-testid="page-mark-image" src={mark.url} alt="" className="size-7 rounded-(--radius-control)" />;
  }
  return (
    <span aria-hidden="true" data-testid="page-mark-glyph" className="text-(--text-muted)">
      ¶
    </span>
  );
}

export interface PageMarkButtonProps {
  /** Stored `logo_props` of the page. */
  logoProps: Record<string, unknown>;
  /** Read-only viewers get the glyph only. */
  editable: boolean;
  /** The title holds no text: the affordance stays visible unprompted. */
  titleEmpty: boolean;
  /** Persist the chosen mark (an empty record clears it). */
  onPick: (draft: Record<string, unknown>) => void;
}

export function PageMarkButton({ logoProps, editable, titleEmpty, onPick }: PageMarkButtonProps): React.ReactElement {
  const draft = markDraftOf(logoProps);
  if (!editable) {
    return (
      <div
        data-testid="page-mark"
        data-readonly="true"
        className={`${PAGE_MARK_AREA} text-h2 inline-flex size-14 items-center justify-center`}
      >
        <PageMarkGlyph logoProps={logoProps} />
      </div>
    );
  }
  return (
    <div
      data-testid="page-mark"
      data-pinned={titleEmpty ? "true" : undefined}
      // The header area is the hover target, so the affordance reveals
      // from anywhere along the top of the page, not only over its tile.
      className={`${PAGE_MARK_AREA} group/mark relative inline-flex`}
    >
      <div
        data-testid="page-mark-affordance"
        data-revealed={titleEmpty ? "true" : undefined}
        className={
          titleEmpty
            ? "opacity-100 transition-opacity"
            : "opacity-0 transition-opacity group-focus-within/mark:opacity-100 group-hover/mark:opacity-100 pointer-coarse:opacity-100"
        }
      >
        <PageMarkPicker
          draft={draft}
          onDraftChange={onPick}
          triggerTestId="page-mark-trigger"
          triggerClassName="flex size-14 cursor-pointer items-center justify-center rounded-(--radius-control) border border-(--border) bg-(--subtle) text-h2 hover:bg-(--bg)"
          triggerLabel={markTriggerLabel(draft)}
        />
      </div>
    </div>
  );
}
