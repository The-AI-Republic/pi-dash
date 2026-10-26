// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Letter and wash colour catalogue (parity/specs/pages.md §5.3,
// PAGE-036). One ordered catalogue feeds both swatch rows, the trigger's
// preview and the active-swatch state, so the two rows can never drift.
// Keys are the stored `data-text-color`/`data-background-color` values;
// the resolved paint comes from a token pair per key so the same choice
// reads correctly in the light, dark and high-contrast themes.

/** The two colour kinds the control carries, in row order. */
export const PAGE_COLOR_KINDS = ["text", "background"] as const;
export type PageColorKind = (typeof PAGE_COLOR_KINDS)[number];

/** One palette entry: the stored key, its name and its paint. */
export interface PageColorChoice {
  key: string;
  label: string;
  /** Painted for the letter swatch (and the trigger's glyph). */
  textColor: string;
  /** Painted for the wash swatch (and the trigger's tile). */
  backgroundColor: string;
}

/**
 * The eight choices both rows offer (spec §5.3). Labels are the swatch
 * names the popover announces; keys are what the body stores.
 */
export const PAGE_COLOR_CHOICES: readonly PageColorChoice[] = [
  {
    key: "gray",
    label: "Gray",
    textColor: "var(--page-color-gray-text)",
    backgroundColor: "var(--page-color-gray-wash)",
  },
  {
    key: "peach",
    label: "Peach",
    textColor: "var(--page-color-peach-text)",
    backgroundColor: "var(--page-color-peach-wash)",
  },
  {
    key: "pink",
    label: "Pink",
    textColor: "var(--page-color-pink-text)",
    backgroundColor: "var(--page-color-pink-wash)",
  },
  {
    key: "orange",
    label: "Orange",
    textColor: "var(--page-color-orange-text)",
    backgroundColor: "var(--page-color-orange-wash)",
  },
  {
    key: "green",
    label: "Green",
    textColor: "var(--page-color-green-text)",
    backgroundColor: "var(--page-color-green-wash)",
  },
  {
    key: "light-blue",
    label: "Light blue",
    textColor: "var(--page-color-light-blue-text)",
    backgroundColor: "var(--page-color-light-blue-wash)",
  },
  {
    key: "dark-blue",
    label: "Dark blue",
    textColor: "var(--page-color-dark-blue-text)",
    backgroundColor: "var(--page-color-dark-blue-wash)",
  },
  {
    key: "purple",
    label: "Purple",
    textColor: "var(--page-color-purple-text)",
    backgroundColor: "var(--page-color-purple-wash)",
  },
];

/** Heading each swatch row carries, in row order (spec §5.3). */
export const PAGE_COLOR_ROW_LABELS: Record<PageColorKind, string> = {
  text: "Letter colors",
  background: "Wash colors",
};

/** Accessible names for one swatch and one row's clear action. */
export function colorSwatchLabel(kind: PageColorKind, choice: PageColorChoice): string {
  return kind === "text" ? `Letter color ${choice.label}` : `Wash color ${choice.label}`;
}

export function colorClearLabel(kind: PageColorKind): string {
  return kind === "text" ? "Clear letter color" : "Clear wash color";
}

/** The paint a swatch of one kind wears. */
export function colorSwatchPaint(kind: PageColorKind, choice: PageColorChoice): string {
  return kind === "text" ? choice.textColor : choice.backgroundColor;
}

/** The palette entry a stored key names, or null when it is unknown. */
export function colorChoiceOf(key: string | null | undefined): PageColorChoice | null {
  if (key === null || key === undefined || key === "") return null;
  return PAGE_COLOR_CHOICES.find((choice) => choice.key === key) ?? null;
}

/**
 * The paint for a stored key. Only a catalogue key has paint: the body
 * styles the eight stored keys and renders any other value as plain
 * text, so the control shows the same thing the range does rather than
 * a colour the document will not keep (PAGE-036).
 */
export function colorPaint(kind: PageColorKind, stored: string | null | undefined): string | null {
  const known = colorChoiceOf(stored);
  return known === null ? null : colorSwatchPaint(kind, known);
}

/** Active swatch state: exactly the stored key lights its own swatch. */
export function colorSwatchActive(choice: PageColorChoice, stored: string | null | undefined): boolean {
  return stored === choice.key;
}
