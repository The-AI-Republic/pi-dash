// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page mark in the detail canvas (parity/specs/pages.md §5.2, PAGE-027).
// The mark sits above the title inside the canvas header: the page's
// glyph with the picker behind it. On a page without a mark the
// affordance fades in when the header is hovered (or something in it has
// focus) and stays pinned while the title is empty, so a new page can
// find it. Once a mark exists its glyph is always drawn and opens the
// picker. Editable pages persist every pick, change and clear through
// the mark mutation; read-only pages show the glyph only.

import { SmilePlus } from "lucide-react";
import * as React from "react";

import { ProjectMarkIcon, projectMark } from "../../../shared/pickers/index.js";
import { PageMarkPicker } from "./PageMarkPicker.js";

/** The canvas header block; also the hover target for the affordance. */
export const PAGE_MARK_AREA = "page-header-container";
export const PAGE_MARK_CHANGE_LABEL = "Change the page mark";
export const PAGE_MARK_EMPTY_LABEL = "Add a page mark";
/** Word the unmarked affordance shows (the mark row's own label). */
export const PAGE_MARK_ADD_TEXT = "Icon";

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

/** Whether a stored mark has a face to draw (an upload does, a blank does not). */
export function pageHasMark(logoProps: Record<string, unknown>): boolean {
  return projectMark(logoProps).kind !== "initial";
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

const MARK_TRIGGER_CLASS =
  "flex size-[56px] cursor-pointer items-center justify-center rounded-(--radius-control) border border-(--border) bg-(--subtle) text-h2 hover:bg-(--bg)";

const ADD_TRIGGER_CLASS =
  "text-body flex h-8 cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) font-medium text-(--text-muted) transition-colors hover:bg-(--subtle) hover:text-(--text)";

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
  const marked = pageHasMark(logoProps);
  if (!editable) {
    return (
      <div
        data-testid="page-mark"
        data-readonly="true"
        aria-hidden={marked ? undefined : true}
        className="inline-flex size-[56px] items-center justify-center"
      >
        {marked ? <PageMarkGlyph logoProps={logoProps} /> : null}
      </div>
    );
  }
  if (marked) {
    return (
      <div data-testid="page-mark" className="inline-flex">
        <PageMarkPicker
          draft={draft}
          onDraftChange={onPick}
          triggerTestId="page-mark-trigger"
          triggerClassName={MARK_TRIGGER_CLASS}
          triggerLabel={markTriggerLabel(draft)}
        >
          <PageMarkGlyph logoProps={logoProps} />
        </PageMarkPicker>
      </div>
    );
  }
  // Without a mark the affordance is the picker's own trigger, faded in
  // by the header's hover and pinned while the title is empty.
  return (
    <div data-testid="page-mark" data-pinned={titleEmpty ? "true" : undefined} className="inline-flex">
      <div
        data-testid="page-mark-affordance"
        data-revealed={titleEmpty ? "true" : undefined}
        className={
          // `transition-all` names the same fade the canvas header drives:
          // a page without a mark shows the affordance on hover, on focus
          // within the header, and pinned while its title is empty.
          titleEmpty
            ? "opacity-100 transition-all duration-200"
            : "opacity-0 transition-all duration-200 group-focus-within/page-header:opacity-100 group-hover/page-header:opacity-100 pointer-coarse:opacity-100"
        }
      >
        <PageMarkPicker
          draft={draft}
          onDraftChange={onPick}
          triggerTestId="page-mark-trigger"
          triggerClassName={ADD_TRIGGER_CLASS}
          triggerLabel={markTriggerLabel(draft)}
        >
          <SmilePlus size={16} strokeWidth={1.75} aria-hidden="true" />
          <span>{PAGE_MARK_ADD_TEXT}</span>
        </PageMarkPicker>
      </div>
    </div>
  );
}
