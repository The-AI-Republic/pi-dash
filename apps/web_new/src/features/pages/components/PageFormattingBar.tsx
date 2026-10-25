// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sticky formatting bar (parity/specs/pages.md §5.3, §9.2, PAGE-028–032).
// A sticky strip above the body holding the typography selector and the
// icon-only controls in the spec's reading order. Each icon control owns
// a tooltip (the action's name, then its shortcut as separate key caps)
// and an active highlight read from the editor's formatting state, so the
// bar never guesses. It renders for editable pages only; the media slice
// (NEWFRONT-841) appends the table, image and colour entries after the
// core twelve, and the shell hides the whole bar read-only.

import { Kbd, Menu, Tooltip } from "@pidash/kit";
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  ChevronDown,
  Code,
  Italic,
  List,
  ListChecks,
  ListOrdered,
  Quote,
  Strikethrough,
  Underline,
} from "lucide-react";
import * as React from "react";

import type { PageBlockKind, PageFormatCommand, PageFormattingState } from "../../../shared/editor/index.js";
import {
  blockKindLabel,
  blockKindOption,
  controlActive,
  PAGE_BLOCK_KINDS,
  PAGE_TOOLBAR_CONTROLS,
  toolbarHintKeys,
  type PageToolbarControl,
} from "../lib/pageToolbar.js";

/** Element id the bar's shell carries (one toolbar per detail page). */
export const PAGE_TOOLBAR_ID = "page-toolbar-container";
export const PAGE_TOOLBAR_CONTENT_CLASS = "page-toolbar-content";
export const PAGE_FORMATTING_BAR_LABEL = "Formatting";
export const PAGE_TYPOGRAPHY_LABEL = "Text style";

/** Highlight an active icon control wears (readable as a filled tile). */
export const PAGE_CONTROL_ACTIVE_CLASS = "bg-layer-transparent-selected";
const PAGE_CONTROL_IDLE_CLASS = "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)";
/** Every icon control is one square tile in the bar's grid. */
const ICON_BUTTON_CLASS = "grid size-7 place-items-center rounded-(--radius-control)";

const ICONS: Record<PageFormatCommand, React.ComponentType<{ size?: number; strokeWidth?: number }>> = {
  bold: Bold,
  italic: Italic,
  underline: Underline,
  strike: Strikethrough,
  alignLeft: AlignLeft,
  alignCenter: AlignCenter,
  alignRight: AlignRight,
  orderedList: ListOrdered,
  bulletList: List,
  taskList: ListChecks,
  blockquote: Quote,
  code: Code,
};

export interface PageFormattingBarProps {
  /** Current formatting state at the caret/selection. */
  state: PageFormattingState;
  /** Fire one formatting command (the bar never writes REST itself). */
  onCommand: (command: PageFormatCommand) => void;
  /** Restyle the block under the caret as a new typography entry. */
  onBlockKind: (kind: PageBlockKind) => void;
  /**
   * Entries the media slice (NEWFRONT-841) appends after the core twelve:
   * table, image and the colour control. Rendering them here keeps the
   * bar's own markup identical before and after that slice.
   */
  mediaControls?: React.ReactNode;
}

export function PageFormattingBar({
  state,
  onCommand,
  onBlockKind,
  mediaControls,
}: PageFormattingBarProps): React.ReactElement {
  return (
    <div
      id={PAGE_TOOLBAR_ID}
      data-testid="page-formatting-bar"
      role="toolbar"
      aria-label={PAGE_FORMATTING_BAR_LABEL}
      // The shell scrolls; the bar stays above the text column while the
      // body moves under it.
      className="sticky top-0 z-10 overflow-x-auto bg-(--bg)"
    >
      <div
        className={`${PAGE_TOOLBAR_CONTENT_CLASS} flex min-h-13 items-center gap-(--space-1) border-b border-(--border) px-(--space-1) py-(--space-1)`}
      >
        <TypographySelector block={state.block} onPick={onBlockKind} />
        <span aria-hidden="true" className="mx-(--space-1) h-5 w-px shrink-0 bg-(--border)" />
        {PAGE_TOOLBAR_CONTROLS.map((control) => (
          <ControlButton
            key={control.command}
            control={control}
            active={controlActive(control.command, state)}
            onPress={() => onCommand(control.command)}
          />
        ))}
        {mediaControls}
      </div>
    </div>
  );
}

function ControlButton({
  control,
  active,
  onPress,
}: {
  control: PageToolbarControl;
  active: boolean;
  onPress: () => void;
}): React.ReactElement {
  const Icon = ICONS[control.command];
  return (
    <Tooltip content={<PageToolbarHint control={control} />}>
      <button
        type="button"
        aria-label={control.label}
        aria-pressed={active}
        data-testid={`page-toolbar-${control.command}`}
        data-active={active ? "true" : undefined}
        // Mouse-down is prevented so the bar keeps the editor's selection:
        // a mousedown would blur the body and collapse the range the
        // command is meant to restyle.
        onMouseDown={(event) => event.preventDefault()}
        onClick={onPress}
        className={
          active
            ? `${ICON_BUTTON_CLASS} ${PAGE_CONTROL_ACTIVE_CLASS} text-(--text)`
            : `${ICON_BUTTON_CLASS} ${PAGE_CONTROL_IDLE_CLASS}`
        }
      >
        <Icon size={14} strokeWidth={1.75} aria-hidden="true" />
      </button>
    </Tooltip>
  );
}

/**
 * A control's tooltip: the action's name on its own line, then its
 * shortcut drawn as one key cap per key (spec §9.2). Quote and code
 * advertise no shortcut, so their tooltip is the name alone.
 */
export function PageToolbarHint({ control }: { control: PageToolbarControl }): React.ReactElement {
  const keys = toolbarHintKeys(control);
  return (
    <span data-testid={`page-toolbar-hint-${control.command}`} className="flex flex-col items-start gap-(--space-1)">
      <span className="font-medium">{control.label}</span>
      {keys === null ? null : <Kbd keys={keys} />}
    </span>
  );
}

export function TypographySelector({
  block,
  onPick,
}: {
  block: PageFormattingState["block"];
  onPick: (kind: PageBlockKind) => void;
}): React.ReactElement {
  const active = blockKindOption(block);
  return (
    <Menu
      trigger={
        <button
          type="button"
          aria-label={PAGE_TYPOGRAPHY_LABEL}
          data-testid="page-toolbar-typography"
          className="text-body flex h-7 min-w-24 shrink-0 cursor-pointer items-center justify-between gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text) hover:bg-(--subtle)"
        >
          <span>{active.label}</span>
          <ChevronDown size={12} strokeWidth={1.75} aria-hidden="true" className="text-(--text-muted)" />
        </button>
      }
      items={PAGE_BLOCK_KINDS.map((entry) => ({
        label: entry.label,
        itemKey: entry.value,
        onSelect: () => onPick(entry.value),
      }))}
    />
  );
}

/** The selector's label for a block; exported for the bar's tests. */
export function formattingBlockLabel(block: PageFormattingState["block"]): string {
  return blockKindLabel(block);
}
