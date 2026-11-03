// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Letter/wash colour control (parity/specs/pages.md §5.3, PAGE-036). The
// trigger previews the selection's current pair: the wash paints the tile
// behind a letter glyph painted in the letter colour. Pressing it opens a
// popover with one row per kind — letter first, then wash — each row a
// clear action followed by the eight named swatches, so a colour can be
// applied to the current range or removed back to plain styling.

import { Popover, Tooltip } from "@pidash/kit";
import { ALargeSmall, Ban } from "lucide-react";
import * as React from "react";

import type { PageColorKind } from "../lib/pageColors.js";
import {
  colorClearLabel,
  colorPaint,
  colorSwatchActive,
  colorSwatchLabel,
  colorSwatchPaint,
  PAGE_COLOR_CHOICES,
  PAGE_COLOR_KINDS,
  PAGE_COLOR_ROW_LABELS,
} from "../lib/pageColors.js";
import { PAGE_MEDIA_CONTROL_LABELS } from "../lib/pageToolbar.js";

export const PAGE_COLOR_TEST_ID = "page-toolbar-color";
export const PAGE_COLOR_PANEL_TEST_ID = "page-color-panel";
export const PAGE_COLOR_ROW_TEST_ID = "page-color-row";

/** The swatch tile's square size and its bound control. */
const SWATCH_CLASS = "size-6 shrink-0 rounded-(--radius-control) border border-(--border)";
const PREVIEW_CLASS = "grid size-6 place-items-center rounded-(--radius-control) border border-(--border)";

export interface PageColorButtonProps {
  /** Colour key the current range's letters carry, if any. */
  textColor: string | null;
  /** Colour key the current range's wash carries, if any. */
  backgroundColor: string | null;
  /** Apply one palette key of the given kind to the range. */
  onPick: (kind: PageColorKind, key: string) => void;
  /** Remove one colour kind from the range. */
  onClear: (kind: PageColorKind) => void;
}

export function PageColorButton({
  textColor,
  backgroundColor,
  onPick,
  onClear,
}: PageColorButtonProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const stored: Record<PageColorKind, string | null> = { text: textColor, background: backgroundColor };
  const textPaint = colorPaint("text", textColor);
  const washPaint = colorPaint("background", backgroundColor);

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="w-auto"
      trigger={
        <button
          type="button"
          aria-label={PAGE_MEDIA_CONTROL_LABELS.color}
          data-testid={PAGE_COLOR_TEST_ID}
          aria-expanded={open}
          // The bar keeps the editor's selection: a mousedown on the
          // trigger would blur the body and collapse the range the colour
          // is meant to restyle.
          onMouseDown={(event) => event.preventDefault()}
          className="text-body flex h-7 shrink-0 items-center gap-(--space-2) rounded-(--radius-control) pr-(--space-2) pl-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
        >
          <span
            data-testid="page-color-preview"
            className={PREVIEW_CLASS}
            style={{ backgroundColor: washPaint ?? "transparent" }}
          >
            <ALargeSmall aria-hidden="true" size={14} strokeWidth={2} style={{ color: textPaint ?? "inherit" }} />
          </span>
          <span>{PAGE_MEDIA_CONTROL_LABELS.color}</span>
        </button>
      }
    >
      <div data-testid={PAGE_COLOR_PANEL_TEST_ID} className="flex flex-col gap-(--space-8)">
        {PAGE_COLOR_KINDS.map((kind) => (
          <ColorRow
            key={kind}
            kind={kind}
            stored={stored[kind]}
            onPick={(key) => {
              onPick(kind, key);
              setOpen(false);
            }}
            onClear={() => {
              onClear(kind);
              setOpen(false);
            }}
          />
        ))}
      </div>
    </Popover>
  );
}

function ColorRow({
  kind,
  stored,
  onPick,
  onClear,
}: {
  kind: PageColorKind;
  stored: string | null;
  onPick: (key: string) => void;
  onClear: () => void;
}): React.ReactElement {
  return (
    <section data-testid={`${PAGE_COLOR_ROW_TEST_ID}-${kind}`} aria-label={PAGE_COLOR_ROW_LABELS[kind]}>
      <p className="text-caption mb-(--space-4) font-semibold text-(--text-muted)">{PAGE_COLOR_ROW_LABELS[kind]}</p>
      <div className="flex items-center gap-(--space-2)">
        <button
          type="button"
          aria-label={colorClearLabel(kind)}
          data-testid={`page-color-clear-${kind}`}
          onMouseDown={(event) => event.preventDefault()}
          onClick={onClear}
          className={`${SWATCH_CLASS} grid place-items-center text-(--text-muted) hover:bg-(--subtle)`}
        >
          <Ban size={14} strokeWidth={1.75} aria-hidden="true" className="lucide-ban" />
        </button>
        {PAGE_COLOR_CHOICES.map((choice) => (
          <Tooltip key={choice.key} content={choice.label}>
            <button
              type="button"
              aria-label={colorSwatchLabel(kind, choice)}
              aria-pressed={colorSwatchActive(choice, stored)}
              data-testid={`page-color-swatch-${kind}-${choice.key}`}
              data-active={colorSwatchActive(choice, stored) ? "true" : undefined}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => onPick(choice.key)}
              className={`${SWATCH_CLASS} ${colorSwatchActive(choice, stored) ? "ring-2 ring-(--accent) ring-offset-1 ring-offset-(--bg)" : ""}`}
              style={{ backgroundColor: colorSwatchPaint(kind, choice) }}
            />
          </Tooltip>
        ))}
      </div>
    </section>
  );
}
