// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sticky formatting bar (parity/specs/pages.md §5.3, §9.2, PAGE-028–032).
// A sticky strip above the body with the typography selector and the
// icon-only controls, in the spec's reading order. Each control carries
// its own tooltip (name plus key hint) and an active highlight read from
// the editor's formatting state, so the bar never guesses. It is rendered
// for editable pages only; the shell withholds it read-only and when the
// sticky preference is off (PAGE-035, media slice).

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
  controlActive,
  blockKindOption,
  PAGE_BLOCK_KINDS,
  PAGE_TOOLBAR_CONTROLS,
  toolbarHintKeys,
  type PageToolbarControl,
} from "../lib/pageToolbar.js";

export const PAGE_FORMATTING_BAR_LABEL = "Formatting";
export const PAGE_TYPOGRAPHY_LABEL = "Text style";

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

/** Active and idle faces of one icon button (active is a filled tile). */
const CONTROL_CLASS = "flex size-7 items-center justify-center rounded-(--radius-control)";
const ACTIVE_CLASS = "bg-(--subtle) text-(--text)";
const IDLE_CLASS = "text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)";

export interface PageFormattingBarProps {
  /** Current formatting state at the caret/selection. */
  state: PageFormattingState;
  /** Fire one formatting command (the bar never writes REST itself). */
  onCommand: (command: PageFormatCommand) => void;
  /** Restyle the block under the caret as a new typography entry. */
  onBlockKind: (kind: PageBlockKind) => void;
  /**
   * Media entries the media slice (NEWFRONT-841) appends after the core
   * twelve: table, image and the colour control. Rendering them here keeps
   * the bar's own markup identical before and after that slice.
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
      data-testid="page-formatting-bar"
      role="toolbar"
      aria-label={PAGE_FORMATTING_BAR_LABEL}
      className="sticky top-0 z-10 flex flex-wrap items-center gap-(--space-1) border-b border-(--border) bg-(--bg) px-(--space-1) py-(--space-1)"
    >
      <TypographySelector block={state.block} onPick={onBlockKind} />
      <span aria-hidden="true" className="mx-(--space-1) h-5 w-px bg-(--border)" />
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
        className={active ? `${CONTROL_CLASS} ${ACTIVE_CLASS}` : `${CONTROL_CLASS} ${IDLE_CLASS}`}
      >
        <Icon size={14} strokeWidth={1.75} aria-hidden="true" />
      </button>
    </Tooltip>
  );
}

/**
 * A control's tooltip: the action's name, then its shortcut as key caps
 * when it advertises one (spec §9.2). Quote and code carry no hint, so
 * their tooltip is the name alone.
 */
export function PageToolbarHint({ control }: { control: PageToolbarControl }): React.ReactElement {
  const keys = toolbarHintKeys(control);
  return (
    <span data-testid={`page-toolbar-hint-${control.command}`} className="flex items-center gap-(--space-2)">
      <span className="text-emphasis">{control.label}</span>
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
          className="text-body flex h-7 min-w-24 cursor-pointer items-center justify-between gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text) hover:bg-(--subtle)"
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
